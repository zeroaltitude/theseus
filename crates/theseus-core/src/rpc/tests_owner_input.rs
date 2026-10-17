//! Whose input a task takes as a steer (theseus-slmj1, rule 1): the owner's
//! own, from a private place, and not a guest's, a job's, a prompt's, or one
//! from a surface that is never a private place.

use theseus_protocol::{DiscordOrigin, TurnSubmitParams};

use crate::approval::{Answerer, Surface};
use crate::rpc::tests::test_core_with;
use crate::tests_places::{ALICE, LAB, OWNER};

fn who(surface: Surface, user: u64, channel: Option<u64>, named: bool) -> Answerer {
    Answerer {
        label: "test".into(),
        surface,
        discord: named.then(|| DiscordOrigin {
            user_id: user.to_string(),
            channel_id: channel.unwrap_or(user + 1).to_string(),
            guild_id: channel.map(|_| "900000000000000001".to_string()),
        }),
    }
}

fn input() -> TurnSubmitParams {
    TurnSubmitParams {
        input: "also check the tide table".into(),
        ..Default::default()
    }
}

#[test]
fn only_the_owners_own_input_from_a_private_place_is_steered() {
    let core = test_core_with("ok", |cfg| {
        cfg.places.owner = Some(vec![format!("discord:{OWNER}")]);
    });
    let (cli, web) = (
        who(Surface::Cli, 0, None, false),
        who(Surface::Web, 0, None, false),
    );
    assert!(core.owner_input(&input(), &cli), "the CLI is the owner's");
    assert!(core.owner_input(&input(), &web), "so is the web UI");
    // A job's shell reaches the socket as the CLI, and says which session.
    let from_job = TurnSubmitParams {
        opened_from: Some("ses_job".into()),
        ..input()
    };
    assert!(!core.owner_input(&from_job, &cli), "a job's input queues");
    let prompt = TurnSubmitParams {
        prompt: Some(theseus_protocol::mcp::McpPromptRef {
            server: "s".into(),
            name: "p".into(),
            arguments: Default::default(),
        }),
        ..input()
    };
    assert!(!core.owner_input(&prompt, &cli), "a prompt is no typing");
    for surface in [Surface::Mcp, Surface::Unnamed] {
        let a = who(surface, 0, None, false);
        assert!(!core.owner_input(&input(), &a), "{surface:?}");
    }
    // Discord: the owner in their DM; not in a shared channel; not a guest;
    // and not a binding that names nobody.
    let dm = who(Surface::Discord, OWNER, None, true);
    assert!(core.owner_input(&input(), &dm), "the owner's DM");
    let shared = who(Surface::Discord, OWNER, Some(LAB), true);
    assert!(!core.owner_input(&input(), &shared), "a shared channel");
    let guest = who(Surface::Discord, ALICE, None, true);
    assert!(!core.owner_input(&input(), &guest), "a guest");
    let unnamed = who(Surface::Discord, OWNER, None, false);
    assert!(!core.owner_input(&input(), &unnamed), "no origin named");
}
