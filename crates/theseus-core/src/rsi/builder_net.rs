//! The self builder's network: the one list of hosts a self session's sandboxed commands may reach
//! (issue 20; the owner's word of 2026-11-15 17:39:14 on relaxing the sandbox for the self loop, and
//! Tabitha's decision of 20:10 on this).
//!
//! A self session's `proc.run` in L1 does **not** carry the operator's `[sandbox] egress`: that list
//! is the operator's own reach (for a person's sessions), a self build holds no secret to use it
//! with, and carrying it made every sandboxed command of the builder a call with a network, which
//! the removal rule (`own_removals`) leaves to the model. So the gate gives a self session's L1
//! call exactly this list, and the removal rule reads the same one: a call whose list is empty, or
//! is this list, is not held for what it removes. A call that names a host beyond it is judged as
//! any call that names a host beyond `[sandbox] egress` is, and the rule leaves it held.
//!
//! **This REMOVES network reach from self builds** (the operator's list); the gate of a self branch
//! runs in its own view and has none (`sandbox::self_build_view`). The list is a closed, fetch-only
//! one, empty today. A package registry the builder needs is added HERE, to [`HOSTS`], and nowhere
//! else; the gate's filter, the view and the removal rule then follow. Design note:
//! `docs/design/self-own-removals.md`.

/// The hosts a self session's sandboxed commands may reach, as `host:port` (a glob on the host is
/// allowed, as in `[sandbox] egress`). Fetch-only means these are package registries and nothing a
/// command could send to. Empty today.
const HOSTS: &[&str] = &[];

/// The self builder's network, as an egress list.
pub fn self_builder_network() -> Vec<String> {
    HOSTS.iter().map(ToString::to_string).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The list is well formed, whatever is put in it: the proxy matches each entry as written.
    #[test]
    fn every_host_is_one_the_proxy_can_match() {
        crate::egress::check(&self_builder_network()).expect("the self builder's network parses");
    }
}
