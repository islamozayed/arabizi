/// Arabizi to Arabic character mappings.
///
/// Design principles:
/// - Each consonant has a PRIMARY mapping (used for the best candidate)
/// - Ambiguous consonants also have ALTERNATIVE mappings for generating variations
/// - Numbers are unambiguous by design (that's the point of Arabizi)
/// - Vowels are context-dependent

/// Consonant mappings — one primary target each (longest patterns first).
pub const CONSONANT_MAPPINGS: &[(&str, &str)] = &[
    // Trigraphs
    ("tch", "تش"),

    // Digraphs
    ("sh", "ش"),
    ("ch", "ش"),
    ("kh", "خ"),
    ("th", "ث"),
    ("dh", "ذ"),
    ("gh", "غ"),
    ("ph", "ف"),

    // Numbers → emphatic/special letters (unambiguous)
    ("2", "ء"),
    ("3'", "غ"),
    ("3", "ع"),
    ("5", "خ"),
    ("6'", "ظ"),
    ("6", "ط"),
    ("7'", "خ"),
    ("7", "ح"),
    ("8", "ق"),
    ("9'", "ظ"),
    ("9", "ص"),

    // Single consonants
    ("b", "ب"),
    ("t", "ت"),
    ("j", "ج"),
    ("d", "د"),
    ("r", "ر"),
    ("z", "ز"),
    ("s", "س"),
    ("f", "ف"),
    ("q", "ق"),
    ("k", "ك"),
    ("l", "ل"),
    ("m", "م"),
    ("n", "ن"),
    ("h", "ه"),
    ("w", "و"),
    ("y", "ي"),
    ("g", "ج"),
    ("x", "خ"),
    ("v", "ف"),
    ("p", "ب"),
];

/// Alternative mappings for ambiguous consonants.
/// Used to generate variation candidates.
/// (arabizi_pattern, primary_arabic, &[alternative_arabic])
pub const AMBIGUOUS_CONSONANTS: &[(&str, &str, &[&str])] = &[
    ("s",  "س", &["ص", "ث"]),
    ("t",  "ت", &["ط", "ث"]),
    ("d",  "د", &["ض", "ذ"]),
    ("z",  "ز", &["ظ", "ذ"]),
    ("h",  "ه", &["ح"]),
    ("g",  "ج", &["غ", "ق"]),     // dialect variation
    ("k",  "ك", &["ق"]),
    ("th", "ث", &["ذ"]),
    ("dh", "ذ", &["ض", "ظ"]),
    ("ch", "ش", &["تش"]),
];

/// Wider table used ONLY by the user-driven letter picker (Cmd+G).
/// Includes everything in `AMBIGUOUS_CONSONANTS` plus vowel/hamza alternatives
/// the user might want to force when the auto-suggester guessed wrong.
/// NOT used for auto-generated swap candidates (would blow up the ranking).
pub const PICKER_ALTERNATIVES: &[(&str, &str, &[&str])] = &[
    // Mirror of AMBIGUOUS_CONSONANTS, with extra alts where useful.
    ("s",  "س", &["ص", "ث"]),
    ("t",  "ت", &["ط", "ث", "ة"]),
    ("d",  "د", &["ض", "ذ"]),
    ("z",  "ز", &["ظ", "ذ"]),
    ("h",  "ه", &["ح", "ة"]),
    ("g",  "ج", &["غ", "ق"]),
    ("k",  "ك", &["ق"]),
    ("th", "ث", &["ذ"]),
    ("dh", "ذ", &["ض", "ظ"]),
    ("ch", "ش", &["تش"]),
    // Vowels and hamza — picker-only.
    ("aa", "ا", &["ى", "آ"]),
    ("ee", "ي", &["ئ"]),
    ("a",  "ا", &["ع", "أ", "إ", "ة"]),
    ("e",  "ا", &["ع", "ي", "ة", "إ"]),
    ("i",  "ي", &["ع", "ئ"]),
    ("o",  "و", &["ؤ"]),
    ("u",  "و", &["ؤ"]),
    ("2",  "ء", &["أ", "إ", "ؤ", "ئ"]),
];

/// Long vowel patterns
pub const LONG_VOWEL_MAPPINGS: &[(&str, &str)] = &[
    ("aa", "ا"),
    ("ee", "ي"),
    ("ii", "ي"),
    ("oo", "و"),
    ("uu", "و"),
    ("ou", "و"),
    ("ei", "ي"),
    ("ai", "ي"),
    ("au", "و"),
];

/// Short vowels
pub const SHORT_VOWELS: &[(&str, &str)] = &[
    ("a", "ا"),
    ("e", "ا"),
    ("i", "ي"),
    ("o", "و"),
    ("u", "و"),
];

pub fn is_vowel_char(c: char) -> bool {
    matches!(c, 'a' | 'e' | 'i' | 'o' | 'u')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_duplicate_consonant_patterns() {
        let mut seen = std::collections::HashSet::new();
        for (pattern, _) in CONSONANT_MAPPINGS {
            assert!(seen.insert(pattern), "Duplicate consonant pattern: {}", pattern);
        }
    }

    #[test]
    fn no_duplicate_long_vowel_patterns() {
        let mut seen = std::collections::HashSet::new();
        for (pattern, _) in LONG_VOWEL_MAPPINGS {
            assert!(seen.insert(pattern), "Duplicate long vowel pattern: {}", pattern);
        }
    }
}
