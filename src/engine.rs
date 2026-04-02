use std::collections::HashMap;

use crate::dictionary::build_dictionary;
use crate::mappings::all_mappings;

/// The main transliteration engine.
/// Converts Arabizi (Franco Arabic) text into Arabic script.
pub struct TransliterationEngine {
    dictionary: HashMap<String, Vec<String>>,
    mappings: Vec<(&'static str, &'static [&'static str])>,
}

impl TransliterationEngine {
    pub fn new() -> Self {
        Self {
            dictionary: build_dictionary(),
            mappings: all_mappings(),
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

        // Build combined results — take top candidate for each word
        // then also provide the top alternative if any word has multiple candidates
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
    /// Returns candidates ordered by likelihood.
    fn transliterate_word(&self, word: &str) -> Vec<String> {
        let word = word.to_lowercase();

        // Dictionary lookup first
        if let Some(candidates) = self.dictionary.get(&word) {
            return candidates.clone();
        }

        // Rule-based transliteration
        self.rule_based_transliterate(&word)
    }

    /// Apply mapping rules to convert an Arabizi word to Arabic.
    /// Uses greedy longest-match from left to right.
    /// Returns multiple candidates when mappings are ambiguous.
    fn rule_based_transliterate(&self, word: &str) -> Vec<String> {
        // Build a sequence of candidate groups for each position
        let char_groups = self.parse_to_groups(word);

        if char_groups.is_empty() {
            return vec![word.to_string()];
        }

        // Generate candidates by combining groups
        // Limit to avoid combinatorial explosion
        self.combine_groups(&char_groups, 5)
    }

    /// Parse input into groups of Arabic character candidates.
    /// Each group represents one matched pattern position.
    fn parse_to_groups(&self, input: &str) -> Vec<Vec<String>> {
        let mut groups = Vec::new();
        let mut pos = 0;
        let chars: Vec<char> = input.chars().collect();
        let len = chars.len();

        while pos < len {
            let remaining: String = chars[pos..].iter().collect();
            let mut matched = false;

            // Try each mapping pattern (already sorted longest-first)
            for (pattern, candidates) in &self.mappings {
                if remaining.starts_with(pattern) {
                    groups.push(candidates.iter().map(|s| s.to_string()).collect());
                    pos += pattern.len();
                    matched = true;
                    break;
                }
            }

            if !matched {
                // Keep the character as-is (punctuation, spaces, etc.)
                groups.push(vec![chars[pos].to_string()]);
                pos += 1;
            }
        }

        groups
    }

    /// Combine groups of candidates into full word candidates.
    /// Limits output to `max_results` to avoid explosion.
    fn combine_groups(&self, groups: &[Vec<String>], max_results: usize) -> Vec<String> {
        let mut results: Vec<String> = vec![String::new()];

        for group in groups {
            let mut new_results = Vec::new();
            for existing in &results {
                for candidate in group {
                    let combined = format!("{}{}", existing, candidate);
                    new_results.push(combined);
                    if new_results.len() >= max_results * 10 {
                        break;
                    }
                }
                if new_results.len() >= max_results * 10 {
                    break;
                }
            }
            results = new_results;
        }

        results.truncate(max_results);
        results
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
    fn rule_based_simple() {
        let e = engine();
        let results = e.transliterate("bism");
        // Should produce something reasonable via rules
        assert!(!results.is_empty());
        // First character should start with ba
        assert!(results[0].starts_with("بـ") || results[0].starts_with("ب"),
            "Expected result starting with ب, got {:?}", results);
    }

    #[test]
    fn multi_word_input() {
        let e = engine();
        let results = e.transliterate("yalla habibi");
        assert!(!results.is_empty());
        // Should contain both words transliterated
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
        // "7" alone should map via rules to ح
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
}
