//! "Did you mean": the closest known name to a misspelt one.

/// Levenshtein distance between two words, counted in characters.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != *cb);
            cur.push((prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// The candidate closest to `word`, if it is within `max(2, word.len() / 3)` edits. The first
/// candidate wins a tie.
pub(crate) fn did_you_mean<'a>(
    word: &str,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Option<&'a str> {
    let limit = 2.max(word.len() / 3);
    let mut best: Option<(usize, &'a str)> = None;
    for c in candidates {
        let d = distance(word, c);
        if d <= limit && best.is_none_or(|(b, _)| d < b) {
            best = Some((d, c));
        }
    }
    best.map(|(_, c)| c)
}

/// Whether `word` is within two edits of one of the candidates.
pub(crate) fn near<'a>(word: &str, candidates: impl IntoIterator<Item = &'a str>) -> bool {
    candidates.into_iter().any(|c| distance(word, c) <= 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_close_name() {
        assert_eq!(
            did_you_mean("rx-tabel", ["rx-table", "rx-card"]),
            Some("rx-table")
        );
    }

    #[test]
    fn far_words_have_no_suggestion() {
        assert_eq!(did_you_mean("zzz", ["rx-table", "rx-card"]), None);
    }

    #[test]
    fn first_wins_a_tie() {
        assert_eq!(did_you_mean("ab", ["ac", "ad"]), Some("ac"));
    }
}
