//! The request's fixed parts (theseus-ev1, theseus-h4a7): the system prompt's
//! blocks and the turn's request spec. Moved out of `turn.rs`, word for
//! word, when thread-context's layers took it past its line ceiling.

use super::*;

impl TurnRunner {
    /// The system prompt, as its three blocks (theseus-ev1, theseus-h4a7's
    /// layers: header, persona, project). The header, which
    /// every session of the profile and their tasks share: the persona, the
    /// assembly note (theseus-fpm2), the tools paragraph, and the profile's
    /// own text. Then the persona's context files, then the system level's,
    /// each under its header naming its level (theseus-58a, theseus-c48);
    /// each empty without files. Deterministic
    /// for a config and the files' contents; a change is a `system_changed`
    /// recompile. Nothing retractable belongs in the header (Appendix F,
    /// theseus-3nk): it is every session's prefix.
    pub fn system_blocks(
        &self,
        target: &Target,
        files: &[ContextFile],
        place: crate::ceiling::PlaceView,
    ) -> (String, String, String) {
        let parts: Vec<String> = self
            .header_parts(
                target,
                place,
                !self.cfg.persona_engaging() && crate::persona::owned(files),
            )
            .into_iter()
            .map(|p| p.1)
            .collect();
        // The persona layer: every persona's files; the project layer: the
        // system level's (`compiler::layers`).
        let layer = |own: bool| {
            let sections: Vec<&str> = files
                .iter()
                .filter(|f| f.file.persona.is_some() == own)
                .map(|f| f.section.as_str())
                .collect();
            sections.join("\n\n")
        };
        (parts.join("\n\n"), layer(true), layer(false))
    }

    /// The turn's request spec, fixed for all its loops, and the context
    /// files this daemon run finds unreadable for the first time. A task's
    /// own conversation is cached for 5 minutes whatever the profile says
    /// (theseus-ev1): its loops run seconds apart, so a 1-hour entry would
    /// only add its write premium. Its header keeps the profile's TTL, the
    /// one its parent's requests write.
    pub fn request_spec(
        &self,
        target: &Target,
        kind: SessionKind,
        place: crate::ceiling::PlaceView,
    ) -> (RequestSpec, Vec<Unreadable>) {
        let paths = self.cfg.context_paths(target.persona.as_deref());
        let (files, unreadable) = self.context_files.load(&paths);
        (self.spec_of(target, kind, place, files), unreadable)
    }
}
