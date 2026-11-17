//! A stand-in authenticator for the tests of the cards that need the owner's
//! second proof (`config_change`, an MCP server's card, the install card):
//! a random secret enrolled the way an enrolment leaves the file, and codes
//! from it. Tests only; the daemon's own code never builds one.

use super::file;
use super::totp::{self, Secret};
use super::{Answer, Proofs};

impl Proofs {
    /// Enrol a random secret, the last step well behind now, so the code of
    /// the current step is good once.
    pub(crate) fn enrol_stand_in(&self) -> Secret {
        use std::io::Read;
        let mut bytes = [0u8; 20];
        std::fs::File::open("/dev/urandom")
            .unwrap()
            .read_exact(&mut bytes)
            .unwrap();
        let secret = Secret::from_bytes(bytes);
        let now = theseus_protocol::now_unix_ms();
        let held = file::Held::new(&secret, now, totp::step_at(now / 1000).saturating_sub(10));
        file::write(&self.path, &held).unwrap();
        secret
    }

    /// The code the stand-in's app shows `ahead` steps from now.
    pub(crate) fn stand_in_code(secret: &Secret, ahead: u64) -> String {
        totp::code_at_step(
            secret,
            totp::step_at(theseus_protocol::now_unix_ms() / 1000) + ahead,
        )
    }

    /// Enrol a stand-in and give a good code from `place`, as the owner's
    /// first code there does: its window is open.
    pub(crate) fn open_stand_in_window(&self, place: &str) -> Secret {
        let secret = self.enrol_stand_in();
        let code = Self::stand_in_code(&secret, 0);
        let answer = Answer {
            code: Some(&code),
            place,
        };
        assert_eq!(
            self.judge("stand-in", true, false, &answer),
            super::Proof::Given
        );
        secret
    }
}
