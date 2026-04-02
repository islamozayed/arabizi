/// Arabizi to Arabic character mappings.
///
/// Design principles:
/// - Each consonant maps to ONE primary Arabic letter (no ambiguity)
/// - Numbers are used specifically for emphatic/special letters (that's the point of Arabizi)
/// - Vowels are context-dependent: short vowels between consonants are dropped,
///   long vowels (aa, ee, oo, etc.) become standalone letters
///
/// The engine uses these mappings; ambiguity is resolved by the dictionary, not here.

/// Consonant mappings — one primary target each.
/// Pattern → Arabic letter.
pub const CONSONANT_MAPPINGS: &[(&str, &str)] = &[
    // Trigraphs
    ("tch", "تش"),

    // Digraphs (must come before single chars)
    ("sh", "ش"),
    ("ch", "ش"),      // common in Maghrebi dialect
    ("kh", "خ"),
    ("th", "ث"),
    ("dh", "ذ"),
    ("gh", "غ"),
    ("ph", "ف"),

    // Numbers → emphatic/special letters (unambiguous by design)
    ("2", "ء"),       // hamza
    ("3'", "غ"),      // ghayn variant
    ("3", "ع"),       // ayn
    ("5", "خ"),       // kha
    ("6'", "ظ"),      // DHa variant
    ("6", "ط"),       // emphatic T
    ("7'", "خ"),      // kha variant
    ("7", "ح"),       // Ha
    ("8", "ق"),       // qaf (Gulf)
    ("9'", "ظ"),      // DHa variant
    ("9", "ص"),       // Sad

    // Single consonants — one mapping each
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
    ("g", "ج"),       // Egyptian: ج
    ("x", "خ"),
    ("v", "ف"),
    ("p", "ب"),
];

/// Long vowel patterns — these always produce a standalone Arabic letter.
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

/// Short vowels — context-dependent.
/// At word start: produce alef + vowel mark.
/// Between consonants: usually dropped (Arabic doesn't write short vowels).
/// At word end: may produce a letter.
pub const SHORT_VOWELS: &[(&str, &str)] = &[
    ("a", "ا"),
    ("e", "ا"),
    ("i", "ي"),
    ("o", "و"),
    ("u", "و"),
];

/// Check if a character is a consonant pattern starter.
pub fn is_consonant_char(c: char) -> bool {
    matches!(c, 'b' | 't' | 'j' | 'd' | 'r' | 'z' | 's' | 'f' | 'q' | 'k' | 'l' | 'm' | 'n' | 'h' | 'w' | 'y' | 'g' | 'x' | 'v' | 'p')
}

pub fn is_vowel_char(c: char) -> bool {
    matches!(c, 'a' | 'e' | 'i' | 'o' | 'u')
}

pub fn is_number_char(c: char) -> bool {
    matches!(c, '2' | '3' | '5' | '6' | '7' | '8' | '9')
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
