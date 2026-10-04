//! Discord end to end without a person, in the gate (theseus-9kjv): the
//! kl8m proof (`theseus_sim::discord_proof`) against this build's
//! `theseusd`. A typed message and its reply, a write on the approve list and
//! its card in a private channel, a press the place refuses, an Approve
//! press, the call run, and the card updated, through stand-ins for
//! Discord's REST and gateway and for the model. Nothing leaves the machine,
//! and no real token is used: a fake `op` answers every secret.

use std::path::PathBuf;

use theseus_sim::discord_proof::{run, Opts};

#[test]
fn discord_works_end_to_end_without_a_person() {
    let r = run(&Opts {
        theseusd: PathBuf::from(env!("CARGO_BIN_EXE_theseusd")),
        dir: None,
        verbose: false,
    })
    .unwrap();
    assert!(r.passed(), "{}", r.render());
    assert_eq!(r.steps.len(), 12, "{}", r.render());
}
