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
         your change did not cause it, say so in your why instead of working around it.\n\
         A small fault your gate or your review finds may be repaired in place by another model, \
         which is no reason to skip running your own tests."
    )
}

/// What a builder is told when its change came back because main had moved (issue 20): its tree
/// holds the finished change with main already merged into it, uncommitted, and what clashes is
/// left for it to resolve. None unless the item's newest word is a tree made so, newer than its
/// last start. The merge's own commit is the builder's `self.propose_change`, and none of this
/// uses one of its tries.
pub fn main_moved(t: &theseus_protocol::tasks::TaskRecord) -> Option<String> {
    let last =
        t.evidence.iter().rev().find(|e| {
            e.identity.starts_with("main-merged:") || e.identity.starts_with("session:")
        })?;
    last.identity.strip_prefix("main-merged:")?;
    let clash = last.note.as_deref().unwrap_or_default();
    Some(format!(
        "Main moved while your finished change waited, and the change no longer merges into it \
         ({clash}). Your tree holds that change with main already merged into it, not yet \
         committed. Open each file that clashes and resolve it: keep what your change set out to \
         do and what main now has, and do not undo main's work. Then run the tests your change \
         touches and call self.propose_change, which commits the merge and checks it again. \
         That is not a failed try, because the change itself was fine."
    ))
}
