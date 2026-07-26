//! Heuristic classifier deciding whether a single-word substitution is a dictionary candidate.
//! Pure. No I/O. Precision-over-recall: a missed name is fine, a misfire is irritating.

const COMMON_WORDS: &[&str] = &[
    "the","be","to","of","and","a","in","that","have","i","it","for","not","on","with","he",
    "as","you","do","at","this","but","his","by","from","they","we","say","her","she","or",
    "an","will","my","one","all","would","there","their","what","so","up","out","if","about",
    "who","get","which","go","me","when","make","can","like","time","no","just","him","know",
    "take","people","into","year","your","good","some","could","them","see","other","than",
    "then","now","look","only","come","its","over","think","also","back","after","use","two",
    "how","our","work","first","well","way","even","new","want","because","any","these","give",
    "day","most","us","is","are","was","were","been","has","had","does","did","being","having",
    "am","yes","ok","okay","please","thanks","thank","hello","hi","hey","yeah","sure",
    "maybe","really","actually","probably","kind","sort","things","thing","stuff","right",
    "left","next","last","fine","great","nice","sorry","again","still","very","much","more",
    "less","few","many","every","always","never","sometimes","often","once","twice","may",
    "should","must","might","let","lets","need","needs","got","gotten","comes","came","goes",
    "went","seen","done","made","said","told","asked","tried","tries","seems","seemed","felt",
    "found","kept","brought","heard","held","stood","sat","ran","met",
];

fn first_char(s: &str) -> Option<char> {
    s.chars().next()
}

fn has_internal_uppercase(s: &str) -> bool {
    let mut chars = s.chars();
    let _ = chars.next();
    chars.any(|c| c.is_uppercase())
}

fn is_all_caps_acronym(s: &str) -> bool {
    s.chars().count() >= 3 && s.chars().all(|c| c.is_uppercase() || c == '-' || c == '\'')
}

fn contains_digit(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_digit())
}

fn only_allowed_chars(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_alphanumeric() || c == '\'' || c == '-')
}

pub fn is_dictionary_candidate(
    old: &str,
    new: &str,
    existing_dictionary_lower: &[String],
) -> bool {
    if new.chars().count() < 3 {
        return false;
    }
    if !only_allowed_chars(new) {
        return false;
    }
    if new == old {
        return false;
    }
    let first = match first_char(new) {
        Some(c) => c,
        None => return false,
    };
    let passes_shape = first.is_uppercase()
        || has_internal_uppercase(new)
        || is_all_caps_acronym(new)
        || contains_digit(new);
    if !passes_shape {
        return false;
    }
    let lower = new.to_lowercase();
    if COMMON_WORDS.contains(&lower.as_str()) {
        return false;
    }
    if existing_dictionary_lower.iter().any(|w| w == &lower) {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty() -> Vec<String> { Vec::new() }

    #[test]
    fn accepts_capitalized_proper_noun() {
        assert!(is_dictionary_candidate("timmy", "Tim", &empty()));
    }

    #[test]
    fn accepts_camel_case() {
        assert!(is_dictionary_candidate("iphone", "iPhone", &empty()));
    }

    #[test]
    fn accepts_all_caps_acronym() {
        assert!(is_dictionary_candidate("aws", "AWS", &empty()));
    }

    #[test]
    fn accepts_alphanumeric_brand() {
        assert!(is_dictionary_candidate("k8s", "K8s", &empty()));
    }

    #[test]
    fn rejects_short() {
        assert!(!is_dictionary_candidate("a", "Hi", &empty()));
    }

    #[test]
    fn rejects_lowercased_common_word() {
        assert!(!is_dictionary_candidate("The", "the", &empty()));
    }

    #[test]
    fn rejects_non_alphabetic() {
        assert!(!is_dictionary_candidate("hi", "hi!", &empty()));
    }

    #[test]
    fn rejects_when_in_dictionary_case_insensitive() {
        let existing = vec!["tim".to_string()];
        assert!(!is_dictionary_candidate("timothy", "Tim", &existing));
    }

    #[test]
    fn accepts_with_hyphen_and_apostrophe() {
        assert!(is_dictionary_candidate("oconnor", "O'Connor", &empty()));
        assert!(is_dictionary_candidate("saint", "Saint-Étienne", &empty()));
    }
}
