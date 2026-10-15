//! IAM's `*` and `?` patterns, and whether one pattern covers another.
//!
//! [`covers`] is the containment check's one primitive: `p` covers `q` when
//! every string `q` matches, `p` matches too. It reads `q`'s own wildcards as
//! characters only a `*` of `p` can take, so it is sound (a pair it accepts
//! is contained) and, for the patterns IAM policies hold, exact.

/// Whether `p` covers `q`. `fold` ignores case, as IAM does for actions.
pub fn covers(p: &str, q: &str, fold: bool) -> bool {
    let (p, q): (Vec<char>, Vec<char>) = if fold {
        (
            p.to_lowercase().chars().collect(),
            q.to_lowercase().chars().collect(),
        )
    } else {
        (p.chars().collect(), q.chars().collect())
    };
    // `seen[i][j]`: the pair (i, j) is already known not to cover.
    let mut dead = vec![vec![false; q.len() + 1]; p.len() + 1];
    walk(&p, &q, 0, 0, &mut dead)
}

fn walk(p: &[char], q: &[char], i: usize, j: usize, dead: &mut [Vec<bool>]) -> bool {
    if dead[i][j] {
        return false;
    }
    let ok = match (p.get(i), q.get(j)) {
        (None, None) => true,
        (None, Some(_)) => false,
        (Some('*'), _) => walk(p, q, i + 1, j, dead) || (j < q.len() && walk(p, q, i, j + 1, dead)),
        (Some(_), None) => false,
        (Some('?'), Some(c)) => *c != '*' && walk(p, q, i + 1, j + 1, dead),
        (Some(a), Some(b)) => a == b && walk(p, q, i + 1, j + 1, dead),
    };
    if !ok {
        dead[i][j] = true;
    }
    ok
}

/// Whether `pattern` matches the literal `s` (no wildcards in `s`).
pub fn matches(pattern: &str, s: &str, fold: bool) -> bool {
    covers(pattern, s, fold)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pattern_covers_what_it_matches_and_the_narrower_patterns() {
        assert!(covers("*", "s3:GetObject", true));
        assert!(covers("s3:*", "s3:Get*", true));
        assert!(covers("s3:Get*", "s3:GetObject", true));
        assert!(covers("s3:GetObject", "s3:getobject", true));
        assert!(covers("arn:aws:s3:::b/*", "arn:aws:s3:::b/hands/*", false));
        assert!(covers("s3:Get?bject", "s3:GetObject", true));
    }

    #[test]
    fn a_pattern_does_not_cover_a_wider_or_other_one() {
        assert!(!covers("s3:GetObject", "s3:Get*", true));
        assert!(!covers("s3:Get*", "s3:*", true));
        assert!(!covers("s3:Get*", "ec2:GetObject", true));
        assert!(!covers("arn:aws:s3:::b/hands/*", "arn:aws:s3:::b/*", false));
        // A `?` takes one character, never a `*`.
        assert!(!covers("s3:Get?", "s3:Get*", true));
        // Resources are case sensitive.
        assert!(!covers("arn:aws:s3:::B/*", "arn:aws:s3:::b/x", false));
    }
}
