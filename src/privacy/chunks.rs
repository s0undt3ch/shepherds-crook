//! Shingles: the 8-word chunks the verbatim index is built from.

pub(crate) const WORDS: usize = 8;

/// Whitespace collapsed to single spaces within one line; case is kept.
pub(crate) fn normalise_line(line: &str) -> String {
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Every run of `WORDS` consecutive words, line by line; a chunk never spans lines.
pub(crate) fn shingles(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        out.extend(words.windows(WORDS).map(|w| w.join(" ")));
    }
    out
}

/// FNV-1a, 64 bits.
pub(crate) fn fnv1a(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shingles_collapse_whitespace_keep_case_and_stay_in_line() {
        let got = shingles("One  two\tthree four five six seven eight nine\nten eleven");
        assert_eq!(
            got,
            vec![
                "One two three four five six seven eight",
                "two three four five six seven eight nine",
            ]
        );
    }

    #[test]
    fn short_lines_give_no_shingles() {
        assert!(shingles("a b c d e f g\nh i j k l m n").is_empty());
        assert!(shingles("").is_empty());
    }

    #[test]
    fn fnv1a_matches_the_reference_vectors() {
        assert_eq!(fnv1a(""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a("a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a("foobar"), 0x8594_4171_f739_67e8);
    }
}
