//! Chunks of at most 512 tokens (M6 §2.2): a long text gets several, and a
//! hit names its chunk.
//!
//! **Tokens, counted without a model.** BM25 needs no model, so a token here
//! is an estimate that runs high for English and for code: a run of letters
//! and digits is one token per six characters (at least one), and any other
//! character that is not a space is one. A word-piece tokenizer splits most
//! English words into one or two pieces, and punctuation into one each, so a
//! chunk under 512 of these is under 512 of its pieces too, short of runs of
//! rare scripts. Step 29c counts with the embedding model's own tokenizer and
//! re-chunks (its stamp changes, so it rebuilds).
//!
//! Chunks break between words (whitespace), and a chunk is a slice of the
//! text, so it reads as written. A word longer than a chunk is cut inside.

/// The most tokens in one chunk.
pub const MAX_TOKENS: usize = 512;

/// The estimated token count of `s` (see the module's notes).
pub fn tokens(s: &str) -> usize {
    let mut n = 0usize;
    let mut run = 0usize;
    for c in s.chars() {
        if c.is_alphanumeric() {
            run += 1;
            continue;
        }
        n += run.div_ceil(6);
        run = 0;
        if !c.is_whitespace() {
            n += 1;
        }
    }
    n + run.div_ceil(6)
}

/// `text` in chunks of at most `max` tokens each, in order. Empty for a text
/// of only whitespace.
pub fn chunks(text: &str, max: usize) -> Vec<&str> {
    let max = max.max(1);
    let mut out = Vec::new();
    // (start, end) of the chunk being built, and its tokens.
    let mut cur: Option<(usize, usize)> = None;
    let mut cur_tokens = 0;
    for (start, word) in words(text) {
        let end = start + word.len();
        let t = tokens(word);
        if t > max {
            // Too long for any chunk: flush, then cut it in pieces.
            if let Some((s, e)) = cur.take() {
                out.push(&text[s..e]);
            }
            cur_tokens = 0;
            out.extend(split_word(word, max));
            continue;
        }
        match cur {
            Some((s, _)) if cur_tokens + t <= max => {
                cur = Some((s, end));
                cur_tokens += t;
            }
            Some((s, e)) => {
                out.push(&text[s..e]);
                cur = Some((start, end));
                cur_tokens = t;
            }
            None => {
                cur = Some((start, end));
                cur_tokens = t;
            }
        }
    }
    if let Some((s, e)) = cur {
        out.push(&text[s..e]);
    }
    out
}

/// The whitespace-separated words of `text`, with their byte offsets.
fn words(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut rest = text;
    let mut base = 0;
    std::iter::from_fn(move || {
        let lead = rest.len() - rest.trim_start().len();
        base += lead;
        rest = &rest[lead..];
        if rest.is_empty() {
            return None;
        }
        let len = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let word = &rest[..len];
        let at = base;
        base += len;
        rest = &rest[len..];
        Some((at, word))
    })
}

/// A word longer than `max` tokens, in pieces of at most `max` each, cut on
/// character boundaries. Counted as it goes (`tokens` of each prefix would
/// be quadratic in a long word, a pasted blob of base64 say).
fn split_word(word: &str, max: usize) -> Vec<&str> {
    // Tokens closed so far, and the letters and digits of the open run.
    let step = |(done, run): (usize, usize), c: char| {
        if c.is_alphanumeric() {
            (done, run + 1)
        } else {
            (done + run.div_ceil(6) + 1, 0)
        }
    };
    let mut out = Vec::new();
    let mut start = 0;
    let mut count = (0, 0);
    for (i, c) in word.char_indices() {
        let next = step(count, c);
        if next.0 + next.1.div_ceil(6) > max && i > start {
            out.push(&word[start..i]);
            start = i;
            count = step((0, 0), c);
        } else {
            count = next;
        }
    }
    if start < word.len() {
        out.push(&word[start..]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_run_high_for_words_and_count_each_mark() {
        assert_eq!(tokens(""), 0);
        assert_eq!(tokens("port 7433"), 2);
        assert_eq!(tokens("a.b"), 3);
        assert_eq!(tokens("internationalization"), 4);
        assert_eq!(tokens("fn main() {}"), 6);
    }

    #[test]
    fn a_short_text_is_one_chunk_and_whitespace_none() {
        assert_eq!(chunks("hello world", MAX_TOKENS), vec!["hello world"]);
        assert!(chunks("  \n\t ", MAX_TOKENS).is_empty());
    }

    #[test]
    fn a_long_text_is_cut_between_words_under_the_limit() {
        let text: String = (0..2000).map(|i| format!("word{i} ")).collect();
        let cs = chunks(&text, MAX_TOKENS);
        assert!(cs.len() >= 4, "{} chunks", cs.len());
        for c in &cs {
            assert!(tokens(c) <= MAX_TOKENS, "{} tokens", tokens(c));
            assert!(!c.starts_with(' ') && !c.ends_with(' '));
        }
        // Nothing lost, nothing doubled: the words, in order.
        let back: Vec<&str> = cs.iter().flat_map(|c| c.split_whitespace()).collect();
        let want: Vec<&str> = text.split_whitespace().collect();
        assert_eq!(back, want);
    }

    #[test]
    fn a_word_longer_than_a_chunk_is_cut_inside() {
        let long = "x".repeat(6 * 1000);
        let text = format!("before {long} after");
        let cs = chunks(&text, MAX_TOKENS);
        assert_eq!(cs.first(), Some(&"before"));
        assert_eq!(cs.last(), Some(&"after"));
        for c in &cs {
            assert!(tokens(c) <= MAX_TOKENS);
        }
        assert_eq!(cs[1..cs.len() - 1].concat(), long);
        // Multi-byte characters are never cut through.
        let wide = "é".repeat(5000);
        for c in chunks(&wide, 100) {
            assert!(tokens(c) <= 100);
        }
    }
}
