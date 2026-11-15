//! `/project item list` answers in a private place alone (issue 13, review round 2's B3): the list is a read the
//! core does not judge, and it names the owner's work, so a shared place is told the list is private and the
//! core is never asked. Through the interaction path, in a rig whose `#lab` is bound private and in one whose
//! `#lab` is shared.

use theseus_core::approval::{Answerer, Surface};
use theseus_core::provider::Scripted;
use theseus_protocol::projects::{ProjectCreateParams, ProjectItemAddParams};
use theseus_sim::fake_discord::commands::Sender;

use crate::tests_gateway::{Rig, ANA, LAB};
use crate::tests_task_threads::rig;
use crate::tests_war_room::answered;

const ITEM: &str = "Book the survey boat";

/// The owner at the CLI on this machine, who makes the project and its item.
fn owner() -> Answerer {
    Answerer {
        label: "test".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    }
}

/// A rig whose `#lab` is bound `private` or shared, and a project in it with one item.
async fn rig_with_an_item(private: bool) -> Rig {
    let r = rig(Vec::<Scripted>::new(), |_| vec![], private).await;
    r.core
        .project_create(
            &ProjectCreateParams {
                title: "Harbour survey".into(),
                review: Some("none".into()),
                ..Default::default()
            },
            owner(),
        )
        .unwrap();
    r.core
        .project_item_add(
            &ProjectItemAddParams {
                project: "Harbour survey".into(),
                title: ITEM.into(),
                ..Default::default()
            },
            owner(),
        )
        .unwrap();
    r
}

/// ana types `/project item list project:Harbour survey` in `#lab`; what the command answered.
async fn typed_in_lab(r: &Rig) -> String {
    let from = Sender {
        user: ANA,
        name: "ana",
        channel: Some(LAB),
    };
    let iid = r
        .fake
        .slash_in_group(
            &from,
            "project",
            ("item", "list"),
            &[("project", "Harbour survey")],
        )
        .unwrap();
    answered(r, &iid).await
}

/// KEY (B3): a shared place is told the list is private, and the item's title is nowhere in what it was
/// answered. With the rule gone the command lists the project's items there.
#[tokio::test]
async fn a_shared_place_is_not_told_a_projects_items() {
    let r = rig_with_an_item(false).await;
    let said = typed_in_lab(&r).await;
    assert_eq!(said, "A project's items are private.", "{said}");
    assert!(!said.contains(ITEM), "{said}");
}

/// The same command in a private place lists them, so the refusal above is the place's and not the command's.
#[tokio::test]
async fn a_private_place_is_told_a_projects_items() {
    let r = rig_with_an_item(true).await;
    let said = typed_in_lab(&r).await;
    assert!(said.contains(ITEM), "{said}");
    assert_ne!(said, "A project's items are private.", "{said}");
}
