/// Arabizi to Arabic character mappings.
///
/// Mappings are ordered longest-first so the engine can greedily match
/// multi-character sequences (e.g. "sh" before "s").
///
/// Each entry is (arabizi_pattern, &[arabic_candidates]).
/// Multiple candidates handle ambiguity (e.g. "s" could be س or ص).

// Numbers used as Arabic letters
pub const NUMBER_MAPPINGS: &[(&str, &[&str])] = &[
    ("2", &["ء"]),        // hamza
    ("3'", &["غ"]),       // ghayn (3 + apostrophe variant)
    ("3a", &["ع"]),       // ayn followed by alef
    ("3", &["ع"]),        // ayn
    ("5", &["خ"]),        // kha
    ("6", &["ط"]),        // Ta (emphatic t)
    ("7'", &["خ"]),       // kha (variant)
    ("7", &["ح"]),        // Ha
    ("8", &["ق"]),        // qaf (some dialects)
    ("9'", &["ظ"]),       // DHa (variant)
    ("9", &["ص"]),        // Sad
];

// Multi-character sequences (digraphs/trigraphs) — must be checked before singles
pub const MULTI_CHAR_MAPPINGS: &[(&str, &[&str])] = &[
    // Trigraphs
    ("tch", &["تش"]),     // tch → taa + sheen
    ("dha", &["ضا", "ذا"]),
    ("tha", &["ثا", "طا"]),
    ("sha", &["شا"]),
    ("kha", &["خا"]),
    ("gha", &["غا"]),

    // Digraphs
    ("sh", &["ش"]),       // sheen
    ("ch", &["تش", "ش"]), // could be tsh or sh depending on dialect
    ("kh", &["خ"]),       // kha
    ("th", &["ث", "ذ"]),  // could be tha or dhal
    ("dh", &["ذ", "ض"]),  // could be dhal or Dad
    ("gh", &["غ"]),       // ghayn
    ("ph", &["ف"]),       // fa (used by some for emphasis)

    // Common vowel combinations
    ("ou", &["و"]),       // waw
    ("oo", &["و"]),       // long u
    ("ee", &["ي"]),       // long i
    ("aa", &["ا", "آ"]),  // long a / alef madda
    ("ai", &["اي", "ع"]),
    ("ei", &["اي"]),
    ("au", &["او"]),
    ("ii", &["ي"]),       // long i variant
    ("uu", &["و"]),       // long u variant
];

// Single character mappings
pub const SINGLE_CHAR_MAPPINGS: &[(&str, &[&str])] = &[
    // Consonants
    ("b", &["ب"]),
    ("t", &["ت", "ط"]),   // could be ta or Ta
    ("g", &["ج", "غ", "ق"]), // varies by dialect: geem, ghayn, or qaf
    ("j", &["ج"]),
    ("d", &["د", "ض"]),   // could be dal or Dad
    ("r", &["ر"]),
    ("z", &["ز", "ظ"]),   // could be zayn or DHa
    ("s", &["س", "ص"]),   // could be seen or Sad
    ("f", &["ف"]),
    ("q", &["ق"]),
    ("k", &["ك"]),
    ("l", &["ل"]),
    ("m", &["م"]),
    ("n", &["ن"]),
    ("h", &["ه", "ح"]),   // could be ha or Ha
    ("w", &["و"]),
    ("y", &["ي"]),
    ("x", &["خ", "كس"]), // sometimes used for kha or ks
    ("v", &["ف"]),        // used as fa in some dialects
    ("p", &["ب"]),        // Arabic doesn't have p, maps to ba

    // Vowels
    ("a", &["ا", "ع", "أ"]),
    ("e", &["ي", "ا", "إ"]),
    ("i", &["ي", "إ"]),
    ("o", &["و", "أ"]),
    ("u", &["و", "أ"]),
];

/// Returns all mapping tables in priority order (longest match first).
/// Number mappings → multi-char → single char.
pub fn all_mappings() -> Vec<(&'static str, &'static [&'static str])> {
    let mut mappings = Vec::new();
    mappings.extend_from_slice(NUMBER_MAPPINGS);
    mappings.extend_from_slice(MULTI_CHAR_MAPPINGS);
    mappings.extend_from_slice(SINGLE_CHAR_MAPPINGS);

    // Sort by pattern length descending so greedy matching works
    mappings.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
    mappings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mappings_sorted_longest_first() {
        let m = all_mappings();
        for window in m.windows(2) {
            assert!(
                window[0].0.len() >= window[1].0.len(),
                "Mapping '{}' should come before '{}'",
                window[0].0,
                window[1].0
            );
        }
    }

    #[test]
    fn all_mappings_have_candidates() {
        for (pattern, candidates) in all_mappings() {
            assert!(
                !candidates.is_empty(),
                "Pattern '{}' has no candidates",
                pattern
            );
        }
    }
}
