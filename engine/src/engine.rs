use std::collections::HashMap;

use crate::dictionary::build_dictionary;
use crate::mappings::{
    is_vowel_char, AMBIGUOUS_CONSONANTS, CONSONANT_MAPPINGS, LONG_VOWEL_MAPPINGS, SHORT_VOWELS,
};

/// The main transliteration engine.
///
/// Strategy for generating multiple candidates:
/// 1. Dictionary lookup (highest quality, may return multiple)
/// 2. Primary rule-based transliteration (best single guess)
/// 3. Variations via:
///    - Swapping ambiguous consonants (s→ص instead of س, etc.)
///    - Including/excluding short vowels
///    - Keeping all short vowels as standalone letters
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
    pub fn transliterate(&self, input: &str) -> Vec<String> {
        let input = input.trim().to_lowercase();
        if input.is_empty() {
            return vec![];
        }

        if let Some(candidates) = self.dictionary.get(&input) {
            return candidates.clone();
        }

        let words: Vec<&str> = input.split_whitespace().collect();
        if words.len() == 1 {
            return self.transliterate_word(&input);
        }

        let word_candidates: Vec<Vec<String>> = words
            .iter()
            .map(|w| self.transliterate_word(w))
            .collect();

        let mut results = Vec::new();
        let primary: String = word_candidates
            .iter()
            .map(|c| c.first().map(|s| s.as_str()).unwrap_or(""))
            .collect::<Vec<_>>()
            .join(" ");
        results.push(primary);

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

    /// Transliterate a single word, returning multiple candidates.
    pub fn transliterate_word(&self, word: &str) -> Vec<String> {
        let word = word.to_lowercase();
        let mut candidates = Vec::new();

        // 1. Dictionary entries first
        if let Some(dict_entries) = self.dictionary.get(&word) {
            candidates.extend(dict_entries.clone());
        }

        // 2. Primary rule-based (drop short vowels between consonants)
        let primary = self.rule_based_transliterate(&word, VowelMode::DropMiddle);
        if !candidates.contains(&primary) {
            candidates.push(primary);
        }

        // 3. Variation: keep ALL vowels as standalone letters
        let all_vowels = self.rule_based_transliterate(&word, VowelMode::KeepAll);
        if !candidates.contains(&all_vowels) {
            candidates.push(all_vowels);
        }

        // 4. Variation: drop ALL short vowels
        let no_vowels = self.rule_based_transliterate(&word, VowelMode::DropAll);
        if !candidates.contains(&no_vowels) {
            candidates.push(no_vowels);
        }

        // 5. Consonant swap variations — try swapping each ambiguous consonant one at a time
        let swaps = self.generate_consonant_swaps(&word);
        for swap in swaps {
            if !candidates.contains(&swap) {
                candidates.push(swap);
                if candidates.len() >= 8 {
                    break;
                }
            }
        }

        // Cap at 8 candidates
        candidates.truncate(8);
        candidates
    }

    /// Generate variations by swapping one ambiguous consonant at a time.
    fn generate_consonant_swaps(&self, word: &str) -> Vec<String> {
        let chars: Vec<char> = word.chars().collect();
        let len = chars.len();
        let mut results = Vec::new();

        // Find positions of ambiguous consonants
        let mut pos = 0;
        while pos < len {
            let remaining: String = chars[pos..].iter().collect();

            // Check if this position matches an ambiguous consonant
            for (pattern, _primary, alternatives) in AMBIGUOUS_CONSONANTS {
                if remaining.starts_with(pattern) {
                    // For each alternative, rebuild the word with the swap
                    for alt in *alternatives {
                        let mut swapped = String::new();
                        // Characters before this position
                        let prefix: String = chars[..pos].iter().collect();
                        swapped.push_str(&self.rule_based_transliterate_raw(&prefix));
                        // The alternative
                        swapped.push_str(alt);
                        // Characters after this pattern
                        let suffix: String = chars[pos + pattern.len()..].iter().collect();
                        swapped.push_str(&self.rule_based_transliterate_raw(&suffix));

                        if !results.contains(&swapped) {
                            results.push(swapped);
                        }
                        if results.len() >= 5 {
                            return results;
                        }
                    }
                    break;
                }
            }

            // Advance past the current token
            if let Some((pattern, _)) = Self::match_consonant(&remaining) {
                pos += pattern.len();
            } else if let Some((pattern, _)) = Self::match_long_vowel(&remaining) {
                pos += pattern.len();
            } else {
                pos += 1;
            }
        }

        results
    }

    /// Simple pass-through transliteration (no vowel context logic).
    /// Used as a building block for consonant swap generation.
    fn rule_based_transliterate_raw(&self, input: &str) -> String {
        self.rule_based_transliterate(input, VowelMode::DropMiddle)
    }

    /// Context-aware rule-based transliteration with configurable vowel handling.
    fn rule_based_transliterate(&self, word: &str, vowel_mode: VowelMode) -> String {
        let chars: Vec<char> = word.chars().collect();
        let len = chars.len();
        let mut result = String::new();
        let mut pos = 0;

        while pos < len {
            let remaining: String = chars[pos..].iter().collect();

            // 1. Consonant mappings
            if let Some((pattern, arabic)) = Self::match_consonant(&remaining) {
                result.push_str(arabic);
                pos += pattern.len();
                continue;
            }

            // 2. Long vowel mappings (always kept)
            if let Some((pattern, arabic)) = Self::match_long_vowel(&remaining) {
                result.push_str(arabic);
                pos += pattern.len();
                continue;
            }

            // 3. Short vowels
            if pos < len && is_vowel_char(chars[pos]) {
                let at_start = pos == 0;
                let at_end = pos == len - 1;
                let before_final = pos + 1 == len - 1;

                match vowel_mode {
                    VowelMode::KeepAll => {
                        if at_start {
                            Self::push_initial_vowel(&mut result, chars[pos]);
                        } else if let Some((_, arabic)) = Self::match_short_vowel(&remaining) {
                            result.push_str(arabic);
                        }
                    }
                    VowelMode::DropAll => {
                        if at_start {
                            Self::push_initial_vowel(&mut result, chars[pos]);
                        }
                        // Otherwise drop
                    }
                    VowelMode::DropMiddle => {
                        if at_start {
                            Self::push_initial_vowel(&mut result, chars[pos]);
                        } else if at_end || before_final {
                            if let Some((_, arabic)) = Self::match_short_vowel(&remaining) {
                                result.push_str(arabic);
                            }
                        }
                        // Otherwise (middle) — drop
                    }
                }
                pos += 1;
                continue;
            }

            // 4. Unknown → keep as-is
            result.push(chars[pos]);
            pos += 1;
        }

        result
    }

    fn push_initial_vowel(result: &mut String, c: char) {
        match c {
            'i' => result.push_str("إ"),
            'u' => result.push_str("أ"),
            'o' => result.push_str("أ"),
            _ => result.push_str("ا"),
        }
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

#[derive(Clone, Copy)]
enum VowelMode {
    /// Drop short vowels between consonants, keep at start/end/before-final
    DropMiddle,
    /// Keep all short vowels as standalone letters
    KeepAll,
    /// Drop all short vowels (except word-initial)
    DropAll,
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
        assert!(!results.is_empty());
        let first = &results[0];
        assert!(first.contains("ب"), "Expected ب in '{}'", first);
        assert!(first.contains("ح"), "Expected ح in '{}'", first);
    }

    #[test]
    fn multiple_candidates_generated() {
        let e = engine();
        let results = e.transliterate_word("salam");
        assert!(results.len() > 1,
            "Expected multiple candidates for 'salam', got {:?}", results);
    }

    #[test]
    fn ambiguous_consonant_variations() {
        let e = engine();
        let results = e.transliterate_word("sala");
        // Should have a variation with ص instead of س
        let has_sad = results.iter().any(|r| r.contains("ص"));
        assert!(has_sad, "Expected a ص variation for 'sala', got {:?}", results);
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
        let first = &results[0];
        assert!(first.starts_with("ا") || first.starts_with("أ"),
            "Expected word-initial alef in '{}'", first);
    }

    #[test]
    fn max_candidates_capped() {
        let e = engine();
        let results = e.transliterate_word("standard");
        assert!(results.len() <= 8,
            "Expected at most 8 candidates, got {}", results.len());
    }
}
