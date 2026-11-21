//! The model's own look at its view: what each presence is shown.

use super::*;
use crate::view::record::tests::view;

fn views() -> Vec<ViewRecord> {
    vec![ViewRecord::new("c", view(1))]
}

#[test]
fn the_owner_alone_is_shown_every_piece_and_no_text() {
    let said = shown(Present::OwnerAlone, &views());
    assert!(
        said.contains("x:far exchanges far [elsewhere_words]"),
        "{said}"
    );
    assert!(said.contains("x:t1 exchanges t1 [window_words]"), "{said}");
    assert!(said.contains("x:far-old"), "{said}");
    assert!(
        !said.contains(" | ") && !said.contains("not the owner alone"),
        "{said}"
    );
}

#[test]
fn anyone_else_present_is_shown_this_places_pieces_and_a_count_of_the_rest() {
    for present in [Present::PrivateChannel, Present::Shared, Present::Open] {
        let said = shown(present, &views());
        assert!(said.contains("x:t2"), "{said}");
        assert!(
            !said.contains("far") && !said.contains("discord"),
            "{present:?}: {said}"
        );
        assert!(
            said.contains("2 of other places, not listed here"),
            "{said}"
        );
        assert!(said.contains("only this place's pieces"), "{said}");
    }
}

#[test]
fn a_turn_with_no_view_says_so() {
    assert!(shown(Present::OwnerAlone, &[]).contains("no view record"));
}
