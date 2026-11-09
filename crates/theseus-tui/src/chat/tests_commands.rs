//! The slash menu (theseus-vjzt, phase 1): what a person sees and can choose.

use crossterm::event::KeyCode;

use super::commands::COMMANDS;
use super::tests::Rig;

/// The menu shows six rows; Down past the sixth scrolls the list so the
/// chosen row is always one the person can see, and a row says how many
/// more there are.
#[tokio::test(start_paused = true)]
async fn the_chosen_row_is_always_on_screen_and_the_rest_are_counted() {
    let mut r = Rig::new(100, 40).await;
    r.ready().await;
    r.press(KeyCode::Char('/')).await;
    assert!(COMMANDS.len() > 9, "the table is long enough to scroll");
    let last_name = format!("/{} ", COMMANDS[COMMANDS.len() - 1].name);
    assert!(
        !r.text().contains(&last_name),
        "the last command is not among the first rows:\n{}",
        r.text()
    );
    let more = COMMANDS.len() - 6;
    assert!(
        r.text().contains(&format!("{more} more")),
        "the first screen counts what it leaves out:\n{}",
        r.text()
    );
    for _ in 0..8 {
        r.press(KeyCode::Down).await;
    }
    let chosen = COMMANDS[8].name;
    assert!(
        r.text().contains(&format!("/{chosen} ")),
        "the ninth command is chosen and shown:\n{}",
        r.text()
    );
    r.press(KeyCode::Tab).await;
    assert!(
        r.text().contains(&format!("› /{chosen}")),
        "Tab completes what is shown:\n{}",
        r.text()
    );
}

/// Esc closes the list and leaves the line as typed; typing opens it again.
#[tokio::test(start_paused = true)]
async fn esc_closes_the_menu_and_typing_opens_it_again() {
    let mut r = Rig::new(100, 30).await;
    r.ready().await;
    r.type_text("/q").await;
    assert!(r.text().contains("leave; the conversation goes on"));
    r.press(KeyCode::Esc).await;
    assert!(
        !r.text().contains("leave; the conversation goes on"),
        "closed:\n{}",
        r.text()
    );
    assert!(r.text().contains("› /q"), "the line stays:\n{}", r.text());
    r.press(KeyCode::Char('u')).await;
    assert!(
        r.text().contains("leave; the conversation goes on"),
        "open again:\n{}",
        r.text()
    );
}

/// Forward-delete is an edit like any other: after Esc closed the list, it
/// opens it again.
#[tokio::test(start_paused = true)]
async fn forward_delete_opens_a_closed_menu_again() {
    let mut r = Rig::new(100, 30).await;
    r.ready().await;
    r.type_text("/quit").await;
    r.press(KeyCode::Left).await;
    r.press(KeyCode::Esc).await;
    assert!(
        !r.text().contains("leave; the conversation goes on"),
        "closed:\n{}",
        r.text()
    );
    r.press(KeyCode::Delete).await;
    assert!(
        r.text().contains("leave; the conversation goes on"),
        "open again:\n{}",
        r.text()
    );
}
