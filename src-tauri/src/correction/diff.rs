//! Pure word-level diff for detecting single-word substitutions inside a typed span.
//!
//! No I/O, no logging, no globals — every behavior is exercised by the tests below.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordSubstitution {
    pub old: String,
    pub new: String,
}

#[derive(Debug, Clone)]
enum Tok<'a> {
    Word(&'a str),
    Other(()),
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '\'' || c == '-'
}

fn tokenize(s: &str) -> Vec<Tok<'_>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let rest = &s[i..];
        let mut chars = rest.char_indices();
        let (_, first) = chars.next().unwrap();
        let is_word = is_word_char(first);
        let mut end = i + first.len_utf8();
        for (off, c) in chars {
            let abs = i + off;
            if is_word_char(c) == is_word {
                end = abs + c.len_utf8();
            } else {
                end = abs;
                break;
            }
        }
        let slice = &s[i..end];
        if is_word {
            out.push(Tok::Word(slice));
        } else {
            out.push(Tok::Other(()));
        }
        i = end;
    }
    out
}

pub fn find_single_word_substitution(
    baseline: &str,
    current: &str,
    typed_text: &str,
) -> Option<WordSubstitution> {
    let base_idx = baseline.find(typed_text)?;
    let typed_end = base_idx + typed_text.len();

    let prefix_anchor = {
        let start = base_idx.saturating_sub(8);
        &baseline[start..base_idx]
    };
    let suffix_anchor = {
        let end = (typed_end + 8).min(baseline.len());
        &baseline[typed_end..end]
    };

    let cur_start = if prefix_anchor.is_empty() {
        0
    } else {
        current.find(prefix_anchor)? + prefix_anchor.len()
    };

    let cur_end = if !suffix_anchor.is_empty() {
        current[cur_start..].find(suffix_anchor).map(|off| cur_start + off)?
    } else {
        current.len()
    };

    let baseline_span = &baseline[base_idx..typed_end];
    let current_span = &current[cur_start..cur_end];

    let b_words: Vec<&str> = tokenize(baseline_span)
        .into_iter()
        .filter_map(|t| match t {
            Tok::Word(w) => Some(w),
            Tok::Other(_) => None,
        })
        .collect();
    // Collect ALL word tokens from current_span (may include tail after the span).
    let all_c_words: Vec<&str> = tokenize(current_span)
        .into_iter()
        .filter_map(|t| match t {
            Tok::Word(w) => Some(w),
            Tok::Other(_) => None,
        })
        .collect();

    let n = b_words.len();

    // current must have at least as many words as baseline (deletions are rejected).
    if all_c_words.len() < n {
        return None;
    }

    // Compare the first N words of current against baseline words.
    let c_words = &all_c_words[..n];
    let mut diff_pos: Option<usize> = None;
    let mut diff_count = 0usize;
    for (i, (bw, cw)) in b_words.iter().zip(c_words.iter()).enumerate() {
        if bw != cw {
            diff_count += 1;
            if diff_count > 1 {
                return None;
            }
            diff_pos = Some(i);
        }
    }

    let i = diff_pos?; // None means no change

    // Guard against word insertion (rather than substitution): if the old word
    // still appears anywhere after position i in all_c_words, the old word was
    // shifted by an insertion — not substituted.
    let old_word = b_words[i];
    if all_c_words[i + 1..].contains(&old_word) {
        return None;
    }

    Some(WordSubstitution {
        old: old_word.to_string(),
        new: c_words[i].to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_single_word_substitution() {
        let baseline = "Hello Timmy how are you ";
        let current = "Hello Tim how are you ";
        let got = find_single_word_substitution(baseline, current, "Hello Timmy how are you ");
        assert_eq!(got, Some(WordSubstitution { old: "Timmy".into(), new: "Tim".into() }));
    }

    #[test]
    fn detects_substitution_with_outside_edits() {
        let baseline = "Hi Timmy. ";
        let current = "Hi Tim, my friend. ";
        let got = find_single_word_substitution(baseline, current, "Hi Timmy. ");
        assert_eq!(got, Some(WordSubstitution { old: "Timmy".into(), new: "Tim".into() }));
    }

    #[test]
    fn rejects_two_word_substitution() {
        let baseline = "Hello Timmy how are you ";
        let current = "Hello Tim how were you ";
        assert!(find_single_word_substitution(baseline, current, baseline).is_none());
    }

    #[test]
    fn rejects_punctuation_only_change() {
        assert!(find_single_word_substitution("Tim. ", "Tim! ", "Tim. ").is_none());
    }

    #[test]
    fn rejects_full_rewrite() {
        assert!(find_single_word_substitution(
            "Hello Timmy how are you ",
            "Completely different sentence ",
            "Hello Timmy how are you ",
        )
        .is_none());
    }

    #[test]
    fn rejects_no_change() {
        assert!(find_single_word_substitution("Tim. ", "Tim. ", "Tim. ").is_none());
    }

    #[test]
    fn handles_unicode_words() {
        let baseline = "I love München ";
        let current = "I love Munich ";
        let got = find_single_word_substitution(baseline, current, baseline);
        assert_eq!(got, Some(WordSubstitution { old: "München".into(), new: "Munich".into() }));
    }

    #[test]
    fn rejects_word_addition_inside_span() {
        assert!(find_single_word_substitution(
            "Hello Tim ",
            "Hello dear Tim ",
            "Hello Tim ",
        )
        .is_none());
    }

    #[test]
    fn rejects_word_deletion_inside_span() {
        assert!(find_single_word_substitution(
            "Hello dear Tim ",
            "Hello Tim ",
            "Hello dear Tim ",
        )
        .is_none());
    }
}
