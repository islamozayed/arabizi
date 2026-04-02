use std::collections::HashMap;

use crate::dictionary::build_dictionary;
use crate::mappings::{
    is_vowel_char, CONSONANT_MAPPINGS, LONG_VOWEL_MAPPINGS, SHORT_VOWELS,
};

/// The main transliteration engine.
/// Converts Arabizi (Franco Arabic) text into Arabic script.
///
/// Strategy:
/// 1. Dictionary lookup for known words (highest quality)
/// 2. Context-aware rule-based transliteration:
///    - Consonants map 1:1 (no ambiguity)
///    - Long vowels (aa, ee, oo) → standalone Arabic letters
///    - Short vowels between consonants → dropped (Arabic omits short vowels)
///    - Short vowels at word start/end → standalone letters
pub struct TransliterationEngine {
    dictionary: HashMap<String, Vec<String>>,
}

impl TransliterationEngine {
    pub fn new() -> Self {
        Self {
            dictionary: build_dictionary(),
        }
    }

    /// Transliterate a full input string (may contain multiple words).
    /// Returns a list of candidate translations, best first.
    pub fn transliterate(&self, input: &str) -> Vec<String> {
        let input = input.trim().to_lowercase();
        if input.is_empty() {
            return vec![];
        }

        // 1. Try full phrase dictionary lookup
        if let Some(candidates) = self.dictionary.get(&input) {
            return candidates.clone();
        }

        // 2. Split into words, transliterate each
        let words: Vec<&str> = input.split_whitespace().collect();
        if words.len() == 1 {
            return self.transliterate_word(&input);
        }

        // For multi-word input, transliterate each word and combine
        let word_candidates: Vec<Vec<String>> = words
            .iter()
            .map(|w| self.transliterate_word(w))
            .collect();

        // Build combined results
        let mut results = Vec::new();

        // Primary: best candidate for each word
        let primary: String = word_candidates
            .iter()
            .map(|candidates| candidates.first().map(|s| s.as_str()).unwrap_or(""))
            .collect::<Vec<_>>()
            .join(" ");
        results.push(primary);

        // Alternatives: swap one word at a time with its second candidate
        for (i, candidates) in word_candidates.iter().enumerate() {
            if candidates.len() > 1 {
                let alt: String = word_candidates
                    .iter()
                    .enumerate()
                    .map(|(j, cands)| {
                        if i == j {
                            cands.get(1).map(|s| s.as_str()).unwrap_or("")
                        } else {
                            cands.first().map(|s| s.as_str()).unwrap_or("")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                results.push(alt);
            }
        }

        results
    }

    /// Transliterate a single word.
    pub fn transliterate_word(&self, word: &str) -> Vec<String> {
        let word = word.to_lowercase();

        // Dictionary lookup first
        if let Some(candidates) = self.dictionary.get(&word) {
            return candidates.clone();
        }

        // Rule-based transliteration
        vec![self.rule_based_transliterate(&word)]
    }

    /// Context-aware rule-based transliteration.
    /// Produces a single best result (no ambiguity explosion).
    fn rule_based_transliterate(&self, word: &str) -> String {
        let chars: Vec<char> = word.chars().collect();
        let len = chars.len();
        let mut result = String::new();
        let mut pos = 0;

        while pos < len {
            let remaining: String = chars[pos..].iter().collect();

            // 1. Try consonant mappings (longest first — they're ordered that way)
            if let Some((pattern, arabic)) = Self::match_consonant(&remaining) {
                result.push_str(arabic);
                pos += pattern.len();
                continue;
            }

            // 2. Try long vowel mappings
            if let Some((pattern, arabic)) = Self::match_long_vowel(&remaining) {
                result.push_str(arabic);
                pos += pattern.len();
                continue;
            }

            // 3. Handle short vowels with context
            if pos < len && is_vowel_char(chars[pos]) {
                let at_start = pos == 0;
                let at_end = pos == len - 1;
                // Check if next non-vowel position is end of word
                let before_final_consonant = pos + 1 == len - 1;

                if at_start {
                    // Word-initial vowel → alef variant
                    match chars[pos] {
                        'i' => result.push_str("إ"),
                        'u' => result.push_str("أ"),
                        'o' => result.push_str("أ"),
                        _ => result.push_str("ا"),  // a, e
                    }
                    pos += 1;
                } else if at_end {
                    // Word-final vowel → standalone letter
                    if let Some((_, arabic)) = Self::match_short_vowel(&remaining) {
                        result.push_str(arabic);
                    }
                    pos += 1;
                } else if before_final_consonant {
                    // Vowel before final consonant — keep it (e.g., the 'a' in "kitab")
                    if let Some((_, arabic)) = Self::match_short_vowel(&remaining) {
                        result.push_str(arabic);
                    }
                    pos += 1;
                } else {
                    // Short vowel between consonants — drop it
                    pos += 1;
                }
                continue;
            }

            // 4. Unknown character — keep as-is
            result.push(chars[pos]);
            pos += 1;
        }

        result
    }

    fn match_consonant(input: &str) -> Option<(&'static str, &'static str)> {
        for (pattern, arabic) in CONSONANT_MAPPINGS {
            if input.starts_with(pattern) {
                return Some((pattern, arabic));
            }
        }
        None
    }

    fn match_long_vowel(input: &str) -> Option<(&'static str, &'static str)> {
        for (pattern, arabic) in LONG_VOWEL_MAPPINGS {
            if input.starts_with(pattern) {
                return Some((pattern, arabic));
            }
        }
        None
    }

    fn match_short_vowel(input: &str) -> Option<(&'static str, &'static str)> {
        for (pattern, arabic) in SHORT_VOWELS {
            if input.starts_with(pattern) {
                return Some((pattern, arabic));
            }
        }
        None
    }
}

impl Default for TransliterationEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> TransliterationEngine {
        TransliterationEngine::new()
    }

    #[test]
    fn empty_input() {
        let e = engine();
        assert!(e.transliterate("").is_empty());
        assert!(e.transliterate("   ").is_empty());
    }

    #[test]
    fn dictionary_word_habibi() {
        let e = engine();
        let results = e.transliterate("7abibi");
        assert!(results.contains(&"حبيبي".to_string()), "Expected حبيبي in {:?}", results);
    }

    #[test]
    fn dictionary_word_inshallah() {
        let e = engine();
        let results = e.transliterate("inshallah");
        assert!(results.contains(&"إن شاء الله".to_string()), "Expected إن شاء الله in {:?}", results);
    }

    #[test]
    fn dictionary_word_shukran() {
        let e = engine();
        let results = e.transliterate("shukran");
        assert!(results.contains(&"شكرا".to_string()), "Expected شكرا in {:?}", results);
    }

    #[test]
    fn rule_based_saba7() {
        let e = engine();
        let results = e.transliterate("saba7");
        // Should produce صباح via rule-based (s=س, short a dropped, b=ب, short a dropped, 7=ح)
        // Actually: s→س, a at position 1 (between consonants, drop), b→ب, a at position 3 (before final consonant, keep), 7→ح
        assert!(!results.is_empty());
        let first = &results[0];
        assert!(first.contains("ب"), "Expected ب in '{}'", first);
        assert!(first.contains("ح"), "Expected ح in '{}'", first);
    }

    #[test]
    fn rule_based_no_ambiguity_explosion() {
        let e = engine();
        let results = e.transliterate("saba7");
        // Should produce exactly 1 result (no ambiguity)
        assert_eq!(results.len(), 1, "Expected 1 result, got {:?}", results);
    }

    #[test]
    fn multi_word_input() {
        let e = engine();
        let results = e.transliterate("yalla habibi");
        assert!(!results.is_empty());
        assert!(results[0].contains("يلا"), "Expected يلا in '{}'", results[0]);
        assert!(results[0].contains("حبيبي"), "Expected حبيبي in '{}'", results[0]);
    }

    #[test]
    fn case_insensitive() {
        let e = engine();
        let lower = e.transliterate("shukran");
        let upper = e.transliterate("SHUKRAN");
        let mixed = e.transliterate("Shukran");
        assert_eq!(lower, upper);
        assert_eq!(lower, mixed);
    }

    #[test]
    fn preserves_unknown_chars() {
        let e = engine();
        let results = e.transliterate("!");
        assert_eq!(results, vec!["!"]);
    }

    #[test]
    fn number_mappings_work() {
        let e = engine();
        let results = e.transliterate_word("7");
        assert!(results.iter().any(|r| r.contains("ح")),
            "Expected ح for '7', got {:?}", results);
    }

    #[test]
    fn digraph_sh() {
        let e = engine();
        let results = e.transliterate_word("sh");
        assert!(results.iter().any(|r| r.contains("ش")),
            "Expected ش for 'sh', got {:?}", results);
    }

    #[test]
    fn word_initial_vowel() {
        let e = engine();
        let results = e.transliterate("ana");
        // "ana" = I in Arabic → انا
        // a(start)→ا, n→ن, a(end)→ا
        let first = &results[0];
        assert!(first.starts_with("ا") || first.starts_with("أ"),
            "Expected word-initial alef in '{}'", first);
    }
}
