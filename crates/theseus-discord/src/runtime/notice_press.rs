//! A press on one of the owner's DM notices: which card's button it is. A
//! child of `runtime`, apart from it for the shape budget's file ceiling.

use theseus_protocol::DiscordOrigin;
use twilight_model::application::interaction::Interaction;

use super::Shared;

impl Shared {
    /// A press on one of the owner's DM notices: Jev's label, a standing
    /// record's, a question's. Whether it was one of them.
    pub(super) async fn notice_press(
        &self,
        i: &Interaction,
        custom_id: &str,
        who: &str,
        discord: Option<DiscordOrigin>,
    ) -> bool {
        if let Some(press) = super::jev::parse(custom_id) {
            self.jev_press(i, press, who, discord).await;
        } else if let Some(press) = super::capture_card::parse(custom_id) {
            self.standing_press(i, press, who, discord).await;
        } else if let Some(press) = super::card_question::parse(custom_id) {
            self.card_question_press(i, press, who, discord).await;
        } else if let Some(press) = super::remind::parse(custom_id) {
            self.remind_press(i, press, who).await;
        } else if let Some(press) = super::seal::parse(custom_id) {
            self.seal_press(i, press, who, discord).await;
        } else if let Some(press) = super::seal::link::parse(custom_id) {
            self.link_press(i, press, who, discord).await;
        } else if let Some(write) = super::onto_card::parse(custom_id) {
            self.onto_press(i, write, who, discord).await;
        } else {
            return self.notice_press_more(i, custom_id, who, discord).await;
        }
        true
    }

    /// The rest of `notice_press`'s chain, apart from it for clippy's complexity limit.
    async fn notice_press_more(
        &self,
        i: &Interaction,
        custom_id: &str,
        who: &str,
        discord: Option<DiscordOrigin>,
    ) -> bool {
        if let Some(press) = super::persona_soul::parse(custom_id) {
            self.persona_soul_press(i, press, who).await;
        } else if let Some(press) = super::content::parse(custom_id) {
            self.content_press(i, press, who, discord).await;
        } else if let Some(press) = super::self_note::parse(custom_id) {
            self.self_note_press(i, press, who).await;
        } else if let Some(press) = super::thread_ask::parse(custom_id) {
            self.thread_ask_press(i, press, who, discord).await;
        } else if let Some(press) = super::ask::parse(custom_id) {
            self.ask_press(i, press, who, discord).await;
        } else if let Some(q) = super::ask::parse_again(custom_id) {
            self.ask_again_press(i, q, who, discord).await;
        } else if let Some(q) = super::ask::parse_details(custom_id) {
            self.ask_details(i, &q, discord).await;
        } else {
            return false;
        }
        true
    }
}
