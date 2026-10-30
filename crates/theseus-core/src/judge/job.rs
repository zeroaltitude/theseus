//! A background job's ask of Jev (route.v10 row 4, theseus-ej6bj).
//!
//! STUB (red commit): no pack asks yet.

use super::JudgeService;

impl JudgeService {
    /// Whether the acting route pack asks the `model` question, so an unset
    /// key asks Jev instead of running its legacy profile.
    pub(crate) fn asks_jev(&self) -> bool {
        false
    }

    /// A test's route pack that asks `model`, standing in for the acting one.
    #[cfg(test)]
    pub(crate) fn job_pack_with(&self, pack: std::sync::Arc<theseus_judge::Pack>) {
        let _ = self.job_pack.set(pack);
    }
}
