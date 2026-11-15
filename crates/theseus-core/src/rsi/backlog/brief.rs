//! The rules a self build is told before it builds (issue 6, condition 1).
//!
//! In one night a change was red for unformatted code, a change broke nine existing tests it
//! had never run, and a review rejected a change as unproved because its tests sat in the same
//! file as the change and went with the planted revert. Each reason was learned one whole gate
//! at a time. The builder is now told the rules first, in plain words, at the head of its
//! brief, and told where it can run the tests its change touches without anyone's answer.

/// The rules, for an item that has `tries` tries.
pub fn rules(tries: u32) -> String {
    format!(
        "Before you build, here is what a change has to meet. A change that does not meet it \
         fails its gate and uses one of your {tries} tries.\n\
         1. Write your code already formatted. Theseus formats it before the gate, but it cannot \
         format a file that does not parse.\n\
         2. Put the tests for your change in a tests file of its own (a file named \
         tests_<what>.rs, or in a tests folder), never in the file you changed. A review undoes \
         your change to see whether your tests fail without it, and tests that sit in the same \
         file go with the change: it then says your change is unproved.\n\
         3. Run the tests your change touches before you call self.propose_change, from your \
         tree, with proc.run and sandbox: true, for example `cargo test -p <crate> <module>`. \
         That runs in your own sandbox and nobody has to answer for it. Run the existing tests \
         of every file you changed too: a change that breaks one has failed.\n\
         4. Theseus does not start a build while main itself is failing. If a test fails and \
         your change did not cause it, say so in your why instead of working around it."
    )
}
