//! Where a card of issue 22 meets the owner's second proof.
//!
//! A change to a sensitive setting, an undo of one, and the install of a
//! joined change are never made on the card's yes alone: each asks for a code
//! from the owner's authenticator app too (`owner_proof`), except as below.
//! The card is **marked** by the daemon's own record, staged in the frame that
//! asks it ([`Needs::mark`]); the one place an answer becomes a decision
//! (`rpc::proof_gate`) then approves a marked question only on a good code or
//! an open window of the answer's own place. The daemon's write asks once more
//! at the yes ([`at_yes`]): the question must carry the mark its settings call
//! for, so a card asked without one is never written.
//!
//! An ordinary change needs the card alone, and so does a sensitive one whose
//! settings `config::sensitivity::proof_reason` does not name (issue 57): prompt
//! text (`PROMPT_TEXT_SHAPES`: `model.system`, `profiles.*.system`,
//! `personas.*.card`), the eight settings the owner's word names
//! (`DROPPED_SHAPES`: `self.mode`, `aws.accounts.*.regions`, `google.account`,
//! `vault.keep_by_discord`, `tools.roots`, `tools.projects_dir`,
//! `lsp.servers.*.roots`, `index.weights_dir`), and tightening one of the five
//! listed kernel limits. Every other sensitive setting keeps the code. Of the MCP
//! server cards, only an attach given a secret, an environment, an address or a
//! network keeps the code; a plain attach and a removal keep their card alone
//! (`config_change::mcp_attach::needs`). A change to who the owner is, to the
//! proof itself, or turning `[self] install` to `"auto"` is marked fresh: a code
//! typed for that change, covered by no window.

use theseus_store::NewRecord;

use super::diff::Setting;
use crate::store::Store;

/// What a card asks of the owner besides the card itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Needs {
    /// An ordinary change: the card's yes.
    Card,
    /// A sensitive change: a code, or an open window of the same place.
    Proof,
    /// A code typed for this change alone, whatever window is open.
    Fresh,
}

impl Needs {
    /// MAIN's requirement, exactly, for a use of a vault secret (`config_change::sensitive_of`):
    /// fresh first, then the code whenever main's sensitivity says so (`sensitive`), else the
    /// card alone. A plain use is not sensitive there, so it is the card alone; any other use
    /// keeps main's code, never this lane's dropped lists.
    pub fn of_main(sensitive: bool, settings: &[Setting]) -> Self {
        match (needs_fresh(settings), sensitive) {
            (true, _) => Self::Fresh,
            (false, true) => Self::Proof,
            (false, false) => Self::Card,
        }
    }

    /// What a change of `settings` asks: fresh when any needs a fresh code, the
    /// proof when any needs the code (`sensitivity::proof_reason`), else the
    /// card alone.
    pub fn of(settings: &[Setting]) -> Self {
        let proof = settings.iter().any(|s| {
            crate::config::sensitivity::proof_reason(&s.path, s.before.as_ref(), s.after.as_ref())
                .is_some()
        });
        match (needs_fresh(settings), proof) {
            (true, _) => Self::Fresh,
            (false, true) => Self::Proof,
            (false, false) => Self::Card,
        }
    }

    /// The record that marks the question `correlation_id`, to be staged in
    /// the frame that asks it. None for a card that asks for no proof.
    pub fn mark(self, correlation_id: &str) -> anyhow::Result<Option<NewRecord>> {
        match self {
            Self::Card => Ok(None),
            Self::Proof => crate::owner_proof::mark(correlation_id, false).map(Some),
            Self::Fresh => crate::owner_proof::mark(correlation_id, true).map(Some),
        }
    }
}

/// The words of a yes that reaches the write on a question the proof never saw.
pub const NOT_ASKED: &str =
    "this change needs the owner's second proof, and the card was not asked \
     with it, so nothing was written: ask for the change again";

/// The daemon's write, before it writes: the question must carry the mark
/// `needs` calls for. The code itself was judged where the answer became a
/// decision; this is the wall behind it, for a card that was made without a
/// mark.
pub fn at_yes(store: &Store, correlation_id: &str, needs: Needs) -> Result<(), String> {
    match (needs, crate::owner_proof::marked(store, correlation_id)) {
        (Needs::Card, _) | (Needs::Proof, Some(_)) | (Needs::Fresh, Some(true)) => Ok(()),
        _ => Err(NOT_ASKED.into()),
    }
}

/// Whether any setting of a change needs a fresh code, one typed for that
/// change alone and covered by no open window (`config::sensitivity::
/// needs_fresh_proof`). Read from the settings that really change, as the
/// sensitive check is.
pub fn needs_fresh(settings: &[Setting]) -> bool {
    settings
        .iter()
        .any(|s| crate::config::sensitivity::needs_fresh_proof(&s.path, s.after.as_ref()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_change::diff;

    fn between(before: &str, after: &str) -> Vec<diff::Setting> {
        diff::between(before, after).expect("both parse")
    }

    #[test]
    fn a_change_that_turns_install_to_auto_needs_a_fresh_proof_however_it_is_written() {
        for after in [
            "[self]\ninstall = \"auto\"\n",
            "self = { install = \"auto\" }\n",
        ] {
            assert!(needs_fresh(&between("", after)), "{after}");
        }
        assert!(needs_fresh(&between(
            "[self]\ninstall = \"off\"\n",
            "[self]\ninstall = \"auto\"\nmode = \"on\"\n"
        )));
    }

    #[test]
    fn the_other_self_settings_and_a_move_away_from_auto_do_not() {
        assert!(!needs_fresh(&between("", "[self]\ninstall = \"ask\"\n")));
        assert!(!needs_fresh(&between(
            "[self]\ninstall = \"auto\"\n",
            "[self]\ninstall = \"off\"\n"
        )));
        assert!(!needs_fresh(&between("", "[self]\nmode = \"on\"\n")));
    }

    #[test]
    fn the_write_refuses_a_question_the_proof_never_saw() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("store")).unwrap();
        // A card that asks for the card alone needs no mark.
        assert_eq!(at_yes(&store, "act_plain", Needs::Card), Ok(()));
        // A question asked with no mark is never written when its settings
        // call for the proof, whatever answered it.
        for needs in [Needs::Proof, Needs::Fresh] {
            assert_eq!(at_yes(&store, "act_bare", needs), Err(NOT_ASKED.into()));
        }
        // Marked for the proof: enough for a sensitive change, not for a fresh one.
        store
            .append(&[
                crate::owner_proof::mark("act_marked", false).unwrap(),
                crate::owner_proof::mark("act_fresh", true).unwrap(),
            ])
            .unwrap();
        assert_eq!(at_yes(&store, "act_marked", Needs::Proof), Ok(()));
        assert_eq!(
            at_yes(&store, "act_marked", Needs::Fresh),
            Err(NOT_ASKED.into())
        );
        assert_eq!(at_yes(&store, "act_fresh", Needs::Fresh), Ok(()));
        assert_eq!(at_yes(&store, "act_fresh", Needs::Proof), Ok(()));
    }

    #[test]
    fn who_the_owner_is_and_the_proof_itself_need_a_fresh_proof() {
        for after in [
            "[places]\nowner = [\"discord:1\"]\n",
            "[discord]\nbindings_file = \"elsewhere.toml\"\n",
            "[server]\nstate_dir = \"/elsewhere\"\n",
            "[owner_proof]\nwindow_hours = 0\n",
        ] {
            assert_eq!(Needs::of(&between("", after)), Needs::Fresh, "{after}");
        }
        let sensitive_only = between("", "[sandbox]\negress = [\"a.example:443\"]\n");
        assert_eq!(Needs::of(&sensitive_only), Needs::Proof);
        assert_eq!(
            Needs::of(&between("", "[import.notes]\nenabled = false\n")),
            Needs::Card
        );
    }

    /// Every case is a setting a real configuration accepts, so the classifier is
    /// read on the schema, not on made-up keys: each is parsed beside the provider's
    /// secret every configuration names.
    fn real(cases: &[&str]) {
        let bad: Vec<String> = cases
            .iter()
            .filter_map(|c| {
                let text = match c.starts_with("[secrets]") {
                    true => c.to_string(),
                    false => format!("[secrets]\nanthropic_api_key = \"op://v/i/f\"\n{c}"),
                };
                crate::Config::parse(&text)
                    .err()
                    .map(|e| format!("{c}: {e:#}"))
            })
            .collect();
        assert!(bad.is_empty(), "not accepted: {bad:#?}");
    }

    /// Each case as a proposal of it alone from an empty file: what its card asks.
    fn of_each(cases: &[&str]) -> Vec<(String, Needs)> {
        real(cases);
        cases
            .iter()
            .map(|c| (c.to_string(), Needs::of(&between("", c))))
            .collect()
    }

    /// (K) Kept, as the owner's yes named: the rules, secrets, the sandbox, places, the broker,
    /// where Theseus listens, its own MCP server, and a key that runs a command.
    #[test]
    fn group_k_keeps_the_code() {
        for (c, n) in of_each(&[
            "[policy.tools]\n\"proc.run\" = \"open\"\n",
            "[secrets]\nanthropic_api_key = \"op://v/i/f\"\nk = \"op://v/i/k\"\n",
            "[sandbox]\npids = 64\n",
            "[places]\npublic_paths = [\"/srv/public\"]\n",
            "[broker.secrets.anthropic_api_key]\nposture = \"approve\"\n",
            "[server]\nmax_connections = 9\n",
            "[mcp_server]\nrequests_per_minute = 30\n",
            "[mcp.servers.files]\ncommand = [\"x\"]\n",
        ]) {
            assert_eq!(n, Needs::Proof, "{c}");
        }
    }

    /// (G1) Kept by the owner's word: an address conversations or messages are sent to, and
    /// which provider a model uses.
    #[test]
    fn group_g1_addresses_keep_the_code_by_his_word() {
        use crate::config::sensitivity::{group, Group};
        for (c, n) in of_each(&[
            "[model]\nprovider = \"anthropic\"\n",
            "[discord]\nrest_proxy = \"proxy.example:80\"\n",
        ]) {
            assert_eq!(n, Needs::Proof, "{c}");
            let s = between("", &c);
            assert_eq!(group(&s[0].path), Some(Group::Addresses), "{c}");
        }
    }

    /// (G2) Kept by the owner's word: the tables that decide or guard later gates.
    #[test]
    fn group_g2_gates_keep_the_code_by_his_word() {
        use crate::config::sensitivity::{group, Group};
        for (c, n) in of_each(&[
            "[judge]\nenabled = false\n",
            "[privacy]\nredact_outbound = false\n",
            "[content_based_security]\ninjection = \"refuse\"\n",
        ]) {
            assert_eq!(n, Needs::Proof, "{c}");
            assert_eq!(group(&between("", &c)[0].path), Some(Group::Gates), "{c}");
        }
    }

    /// The secrets every account case below names, so each case parses alone.
    const SECRETS: &str = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\
                           aws_access_key_id = \"op://v/i/a\"\n\
                           aws_secret_access_key = \"op://v/i/b\"\n\
                           google_client = \"op://v/i/c\"\ngoogle_refresh_token = \"op://v/i/d\"\n";

    /// What a change of `extra`, after [`SECRETS`], asks: only `extra`'s own settings change.
    fn with_secrets(extra: &str) -> Needs {
        let text = format!("{SECRETS}\n{extra}");
        real(&[&text]);
        let s = between(SECRETS, &text);
        assert!(
            !s.is_empty() && s.iter().all(|s| s.path[0] != "secrets"),
            "{s:?}"
        );
        Needs::of(&s)
    }

    /// (G3) Dropped by the owner's word ("I'll take your recommendation"), for EXACTLY the closed
    /// list of shapes: each takes the card alone.
    #[test]
    fn group_g3_is_a_closed_list_of_exact_settings() {
        use crate::config::sensitivity::DROPPED_SHAPES;
        assert_eq!(
            DROPPED_SHAPES.as_slice(),
            &[
                "self.mode",
                "aws.accounts.*.regions",
                "google.account",
                "vault.keep_by_discord",
                "tools.roots",
                "tools.projects_dir",
                "lsp.servers.*.roots",
                "index.weights_dir",
            ][..]
        );
        for (c, n) in of_each(&[
            "[self]\nmode = \"act\"\n",
            "[vault]\nkeep_by_discord = true\n",
            "[tools]\nroots = [\"/x\"]\n",
            "[tools]\nprojects_dir = \"/x\"\n",
            "[lsp.servers.rust-analyzer]\nroots = [\"Cargo.toml\"]\n",
            "[index]\nweights_dir = \"/x\"\n",
        ]) {
            assert_eq!(n, Needs::Card, "{c}");
        }
        assert_eq!(
            with_secrets("[google]\naccount = \"me@example.test\"\n"),
            Needs::Card,
            "google.account"
        );
        let account = "[aws.accounts.111122223333]\nregion = \"us-west-2\"\n";
        assert_eq!(
            Needs::of(&between(
                &format!("{SECRETS}\n{account}"),
                &format!("{SECRETS}\n{account}regions = [\"us-west-2\"]\n")
            )),
            Needs::Card,
            "an AWS account's regions"
        );
    }

    /// The five settings the review cut from G3 keep main's code: Google's calendars (an entry
    /// can carry a `sensitive` flag) and services (they turn services on, sending ones among
    /// them), and an AWS account's durability, deployment and region (they start, stop and
    /// direct the store's shipment to AWS).
    #[test]
    fn the_five_cut_from_g3_keep_mains_code() {
        for (c, n) in of_each(&[
            "[google]\nservices = [\"gmail\"]\n",
            "[google]\ncalendars = [\"primary\"]\n",
        ]) {
            assert_eq!(n, Needs::Proof, "{c}");
        }
        let calendars = |sensitive: bool| {
            format!("[google]\ncalendars = [{{ id = \"primary\", sensitive = {sensitive} }}]\n")
        };
        real(&[&calendars(true), &calendars(false)]);
        assert_eq!(
            Needs::of(&between(&calendars(true), &calendars(false))),
            Needs::Proof,
            "a calendar's sensitive flag"
        );
        let account = "[aws.accounts.111122223333]\nregion = \"us-west-2\"\n";
        assert_eq!(with_secrets(account), Needs::Proof, "an AWS account");
        for (from, to) in [
            ("", "durability = true\n"),
            ("durability = true\n", "durability = false\n"),
            ("deployment = \"a\"\n", "deployment = \"b\"\n"),
        ] {
            assert_eq!(
                Needs::of(&between(
                    &format!("{SECRETS}\n{account}{from}"),
                    &format!("{SECRETS}\n{account}{to}")
                )),
                Needs::Proof,
                "{from} -> {to}"
            );
        }
        assert_eq!(
            Needs::of(&between(
                &format!("{SECRETS}\n{account}"),
                &format!("{SECRETS}\n{}", account.replace("us-west-2", "eu-west-1"))
            )),
            Needs::Proof,
            "an AWS account's region"
        );
    }

    /// (G3) Every setting just outside the closed list keeps main's code: `self.install` (as
    /// `true` too), the rest of `self`, `self_issues`, `self_fix`, an AWS account's hands
    /// network, Google's send limit and trusted senders, an MCP server's `read`, the imported
    /// history's agents, and the persona and context files. The global exceptions win inside the
    /// list's tables, and a mixed change takes the strictest.
    #[test]
    fn the_settings_just_outside_g3_keep_mains_code() {
        let account = "[aws.accounts.111122223333]\nregion = \"us-west-2\"\n";
        for (c, n) in of_each(&[
            "[self]\ninstall = true\n",
            "[self]\nremote = \"elsewhere\"\n",
            "[self]\nreviewer_profile = \"opus\"\n",
            "[self]\nrevert_window_hours = 0\n",
            "[self.sealed]\nevery_branch = true\n",
            "[self_issues]\nenabled = true\n",
            "[self_fix]\nlines = 5000\n",
            "[self_fix]\nfiles = 500\n",
            "[self_fix]\nturn_minutes = 600\n",
            "[google]\nmax_sends_per_day = 500\n",
            "[google]\ntrusted_senders = [\"a@example.test\"]\n",
            "[import.openclaw]\nagents = [\"main\"]\n",
            "[personas.theseus]\nfiles = [\"/x.md\"]\n",
            "[context]\nfiles = [\"/x.md\"]\n",
            "[self.budget]\nday_usd = 3.0\n",
        ]) {
            assert_eq!(n, Needs::Proof, "outside the list: {c}");
        }
        let server = "[mcp.servers.files]\ncommand = [\"x\"]\n";
        let read = format!("{server}read = [\"delete_everything\"]\n");
        real(&[server, &read]);
        assert_eq!(
            Needs::of(&between(server, &read)),
            Needs::Proof,
            "an MCP server's read"
        );
        assert_eq!(
            Needs::of(&between("", "[self]\ninstall = \"auto\"\n")),
            Needs::Fresh,
            "self.install = auto"
        );
        assert_eq!(
            with_secrets(&format!(
                "{account}\n[aws.accounts.111122223333.hands_network]\nvpc = \"vpc-0a1b2c3d4e5f60718\"\n\
                 subnets = [\"subnet-0a1b2c3d4e5f60711\"]\nsecurity_group = \"sg-0a1b2c3d4e5f60718\"\n"
            )),
            Needs::Proof,
            "an AWS account's hands network"
        );
        assert_eq!(
            with_secrets(&format!("{account}daily_budget_usd = 20\n")),
            Needs::Proof,
            "a spending key under aws"
        );
        assert_eq!(
            with_secrets("[google.credentials]\nclient = \"google_client\"\n"),
            Needs::Proof,
            "a key that reads a secret"
        );
        for other in [
            "[model]\nprovider = \"anthropic\"\n",
            "[judge]\nenabled = false\n",
            "[policy.tools]\n\"proc.run\" = \"open\"\n",
            "[self]\ninstall = true\n",
        ] {
            let mixed = format!("[tools]\nroots = [\"/x\"]\n\n{other}");
            real(&[&mixed]);
            assert_eq!(Needs::of(&between("", &mixed)), Needs::Proof, "{mixed}");
        }
    }

    /// (D) Prompt text is EXACTLY the closed list of shapes (a long value included): each takes
    /// the card alone. A key that only has "prompt" or "note" in its name keeps main's code:
    /// `model.end_note` (a switch) and `notify.unprompted_pace_secs` (a pacing limit). Mixed
    /// with a setting that keeps the code, the stricter rule wins.
    #[test]
    fn group_d_prompt_text_is_a_closed_list_of_exact_settings() {
        use crate::config::sensitivity::PROMPT_TEXT_SHAPES;
        assert_eq!(
            PROMPT_TEXT_SHAPES.as_slice(),
            &["model.system", "profiles.*.system", "personas.*.card"][..]
        );
        let long = format!("[model]\nsystem = \"{}\"\n", "be brief. ".repeat(80));
        for (c, n) in of_each(&[
            "[model]\nsystem = \"be brief\"\n",
            &long,
            "[profiles.opus]\nmodel = \"claude-sonnet-5-5\"\nsystem = \"be brief\"\n",
            "[personas.theseus]\ncard = \"a calm voice\"\n",
        ]) {
            assert_eq!(n, Needs::Card, "{c}");
        }
        for (c, n) in of_each(&[
            "[model]\nend_note = false\n",
            "[notify]\nunprompted_pace_secs = 0\n",
        ]) {
            assert_eq!(n, Needs::Proof, "not prompt text: {c}");
        }
        let mixed = "[model]\nsystem = \"be brief\"\n\n[judge]\nenabled = false\n";
        real(&[mixed]);
        assert_eq!(Needs::of(&between("", mixed)), Needs::Proof);
    }

    /// Issue 57, finding 1: a spending limit is compared in the kernel's own units. Dollars
    /// become micro-dollars as `to_kernel_config` converts them (to the nearest one), and zero
    /// micro-dollars is no limit at all: a positive value that converts to zero keeps the code,
    /// as does every change that is not strictly tighter there. The hard multiple is its own
    /// whole-number case. The other spending keys keep the code in either direction.
    #[test]
    fn a_spending_limit_is_compared_in_the_kernels_own_units() {
        use crate::config::sensitivity::{effective, Unit};
        let k = |key: &str, v: &str| format!("[kernel]\n{key} = {v}\n");
        let needs = |b: &str, a: &str| Needs::of(&between(b, a));
        for key in [
            "spend_limit_usd",
            "daily_spend_ceiling_usd",
            "daily_runaway_usd",
            "runaway_usd_per_10min",
        ] {
            let from = k(key, "50.0");
            real(&[&from]);
            assert_eq!(needs(&from, &k(key, "40.0")), Needs::Card, "{key} tighter");
            assert_eq!(needs(&k(key, "40.0"), &from), Needs::Proof, "{key} raised");
            assert_eq!(
                needs(&from, &from.replace("50.0", "50.0000001")),
                Needs::Proof,
                "{key} the same micro-dollars"
            );
            for v in ["0.0000004", "0.0000005", "0.0000006", "0.000001"] {
                let micros = theseus_kernel::usd_to_micros(v.parse().unwrap());
                let want = match micros {
                    0 => Needs::Proof,
                    _ => Needs::Card,
                };
                assert_eq!(
                    needs(&from, &k(key, v)),
                    want,
                    "{key} = {v} ({micros} micro-dollars)"
                );
                let held = effective(Unit::Dollars, &serde_json::json!(v.parse::<f64>().unwrap()));
                assert_eq!(held == Some(u64::MAX), micros == 0, "{key} = {v}");
            }
            assert_eq!(
                needs("", &k(key, "40.0")),
                Needs::Proof,
                "{key} set where there was none"
            );
            assert_eq!(needs(&from, ""), Needs::Proof, "{key} removed");
        }
        let hard = |v: &str| k("spend_limit_hard_multiple", v);
        real(&[&hard("10"), &hard("0")]);
        assert_eq!(needs(&hard("10"), &hard("5")), Needs::Card);
        assert_eq!(
            needs(&hard("10"), &hard("0")),
            Needs::Proof,
            "0 never stops"
        );
        assert_eq!(
            needs(&hard("0"), &hard("5")),
            Needs::Card,
            "from never to a stop"
        );
        assert_eq!(needs(&hard("5"), &hard("10")), Needs::Proof);
        let step = |v: &str| k("daily_notice_step_usd", v);
        assert_eq!(
            needs(&step("200.0"), &step("100.0")),
            Needs::Proof,
            "not a listed limit"
        );
        let day = |v: &str| format!("[self.budget]\nday_usd = {v}\n");
        assert_eq!(
            needs(&day("5.0"), &day("3.0")),
            Needs::Proof,
            "not a listed limit"
        );
        let both = |c: &str, r: &str| {
            format!("[kernel]\ndaily_spend_ceiling_usd = {c}\ndaily_runaway_usd = {r}\n")
        };
        real(&[&both("40.0", "0.0000001")]);
        assert_eq!(
            needs(&both("50.0", "2000.0"), &both("40.0", "0.0000001")),
            Needs::Proof,
            "the stricter rule wins"
        );
        assert_eq!(
            needs(&both("50.0", "2000.0"), &both("40.0", "1000.0")),
            Needs::Card
        );
    }

    /// The catalog's prices decide what a call is charged and reserved: a change to one needs
    /// the code in either direction; a catalog figure that is not a price takes the card alone.
    #[test]
    fn a_catalog_price_needs_the_code_in_either_direction() {
        let row = |k: &str, v: &str| format!("[catalog.\"claude-sonnet-5-5\"]\n{k} = {v}\n");
        let cases = [
            row("input_per_mtok", "3.0"),
            row("input_per_mtok", "0.0"),
            row("input_per_mtok", "9.0"),
            row("cache_write_1h_per_mtok", "0.0"),
            row("vision", "false"),
        ];
        real(&cases.iter().map(String::as_str).collect::<Vec<_>>());
        let needs = |b: &str, a: &str| Needs::of(&between(b, a));
        assert_eq!(needs(&cases[0], &cases[1]), Needs::Proof, "to zero");
        assert_eq!(needs(&cases[0], &cases[2]), Needs::Proof, "raised");
        assert_eq!(
            needs("", &cases[3]),
            Needs::Proof,
            "set where there was none"
        );
        assert_eq!(needs("", &cases[4]), Needs::Card, "not a price");
    }
}
