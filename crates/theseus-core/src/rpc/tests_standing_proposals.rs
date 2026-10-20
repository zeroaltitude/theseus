//! The standing records an imported agent's feedback proposes
//! (theseus-a8n7): the import files proposals and no record; a proposal
//! reaches no prompt until the owner keeps it, and then it is a record for
//! that persona alone; a dismissed one is never proposed again; and a dry run
//! files nothing.

use std::path::Path;

use theseus_protocol::{
    PersonaImportParams, StandingListParams, StandingProposalAnswer, StandingProposalsParams,
    StandingScope,
};

use crate::approval::{Answerer, Surface};
use crate::config::{MemoryMode, PersonaConfig};
use crate::tests_recall::rig_with;

fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    }
}

fn import(dry_run: bool) -> PersonaImportParams {
    PersonaImportParams {
        from: Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/agents/wren")
            .display()
            .to_string(),
        name: "wren".into(),
        dry_run,
        ..Default::default()
    }
}

fn answer(id: &str) -> StandingProposalAnswer {
    StandingProposalAnswer {
        id: id.into(),
        ..Default::default()
    }
}

#[test]
fn feedback_is_proposed_never_live_and_kept_for_its_persona_alone() {
    let r = rig_with(MemoryMode::Off, |c| {
        c.personas.insert("wren".into(), PersonaConfig::default());
        c.personas.insert("finch".into(), PersonaConfig::default());
    });
    let c = &r.core;
    let live = || {
        c.standing_list(&StandingListParams::default(), cli())
            .unwrap()
            .records
    };
    let open = || {
        c.standing_proposals(&StandingProposalsParams::default(), cli())
            .unwrap()
            .proposals
    };

    // A dry run says what it would propose and files nothing.
    let dry = c.persona_import(&import(true)).unwrap();
    assert_eq!(dry.proposals.len(), 3);
    assert!(open().is_empty());

    // The import files three proposals and not one record.
    let done = c.persona_import(&import(false)).unwrap();
    assert_eq!(done.proposals.len(), 3, "{:?}", done.proposals);
    assert!(
        live().is_empty(),
        "an import made a live record: {:?}",
        live()
    );
    let props = open();
    assert_eq!(props.len(), 3);
    assert!(props
        .iter()
        .all(|p| p.persona == "wren" && p.state == "open"));
    let rows = c
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(1000)
        .unwrap();
    assert!(rows.iter().any(|(_, r)| r.is_kind("persona.imported")));

    // Importing again proposes nothing new.
    let again = c.persona_import(&import(false)).unwrap();
    assert!(again.proposals.is_empty() && again.proposals_known == 3);
    assert_eq!(open().len(), 3);

    // A keep is a record for the persona, and only then.
    let metres = props
        .iter()
        .find(|p| p.text.contains("metres"))
        .unwrap()
        .clone();
    let kept = c.standing_keep(&answer(&metres.id[..10]), cli()).unwrap();
    assert_eq!(kept.proposal.state, "kept");
    let rec = kept.record.unwrap();
    assert_eq!(rec.scope, StandingScope::Persona("wren".into()));
    assert_eq!(rec.text, metres.text);
    assert_eq!(rec.source, format!("persona-import:{}", metres.id));
    assert_eq!(live().len(), 1);
    assert_eq!(open().len(), 2);
    let e = c.standing_keep(&answer(&metres.id), cli()).unwrap_err();
    assert!(e.message.contains("already kept"), "{}", e.message);

    // A dismissal makes no record, and the words never come back.
    let brevity = open().into_iter().find(|p| p.text.contains("pad")).unwrap();
    let gone = c.standing_dismiss(&answer(&brevity.id), cli()).unwrap();
    assert!(gone.record.is_none() && gone.proposal.state == "dismissed");
    assert_eq!(live().len(), 1);
    let third = c.persona_import(&import(false)).unwrap();
    assert!(third.proposals.is_empty() && third.proposals_known == 3);
    let all = c
        .standing_proposals(
            &StandingProposalsParams {
                all: true,
                ..Default::default()
            },
            cli(),
        )
        .unwrap()
        .proposals;
    let states: Vec<_> = all.iter().map(|p| p.state.as_str()).collect();
    assert_eq!(
        states.iter().filter(|s| **s == "open").count(),
        1,
        "{states:?}"
    );

    // A proposal's id must name one.
    let e = c.standing_keep(&answer("stp_nosuch"), cli()).unwrap_err();
    assert!(e.message.contains("no open proposal"), "{}", e.message);
}

/// A scope names a persona that exists.
#[test]
fn a_persona_scope_names_a_configured_persona() {
    let r = rig_with(MemoryMode::Off, |c| {
        c.personas.insert("wren".into(), PersonaConfig::default());
    });
    let add = |name: &str| {
        r.core.standing_add(
            &theseus_protocol::StandingAddParams {
                text: "answer in metres".into(),
                scope: StandingScope::Persona(name.into()),
                ..Default::default()
            },
            cli(),
        )
    };
    assert!(add("wren").is_ok());
    let e = add("nobody").unwrap_err();
    assert!(
        e.message.contains("is no persona") && e.message.contains("wren"),
        "{}",
        e.message
    );
}
