use std::collections::HashMap;

use crate::dictionary::build_dictionary;
use crate::emoji::build_emoji_map;
use crate::frequency::FrequencyList;
use crate::mappings::{
    is_vowel_char, AMBIGUOUS_CONSONANTS, CONSONANT_MAPPINGS, LONG_VOWEL_MAPPINGS, SHORT_VOWELS,
};
use crate::user_prefs::UserPreferences;

/// The main transliteration engine.
///
/// Strategy for generating multiple candidates:
/// 1. Dictionary lookup (highest quality, may return multiple)
/// 2. Primary rule-based transliteration (best single guess)
/// 3. Variations via:
///    - Swapping ambiguous consonants (s→ص instead of س, etc.)
///    - Including/excluding short vowels
///    - Keeping all short vowels as standalone letters
///
/// Candidates are then ranked by:
/// 1. User preference (previously selected candidates for this input)
/// 2. Dictionary match bonus
/// 3. Arabic word frequency (common real words rank higher)
pub struct TransliterationEngine {
    dictionary: HashMap<String, Vec<String>>,
    emoji_map: HashMap<String, Vec<&'static str>>,
    frequency: FrequencyList,
}

impl TransliterationEngine {
    pub fn new() -> Self {
        Self {
            dictionary: build_dictionary(),
            emoji_map: build_emoji_map(),
            frequency: FrequencyList::load(),
        }
    }

    /// Look up emojis by checking Arabic candidates against the emoji map.
    /// Tries each candidate in order (best first) and returns the first match.
    pub fn lookup_emojis(&self, candidates: &[String]) -> Vec<String> {
        for candidate in candidates {
            if let Some(emojis) = self.emoji_map.get(candidate) {
                return emojis.iter().map(|e| e.to_string()).collect();
            }
        }
        Vec::new()
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

    /// Transliterate a single word, returning multiple ranked candidates.
    pub fn transliterate_word(&self, word: &str) -> Vec<String> {
        self.transliterate_word_ranked(word, None)
    }

    /// Transliterate a single word with optional user preference ranking.
    pub fn transliterate_word_ranked(&self, word: &str, prefs: Option<&UserPreferences>) -> Vec<String> {
        let word = word.to_lowercase();
        let mut candidates = Vec::new();
        let is_dict_word = self.dictionary.contains_key(&word);

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

        // 5. Tanween variations — "an" ending often means tanween fatha (اً) not literal ان
        let tanween = Self::generate_tanween_variants(&candidates);
        for t in tanween {
            if !candidates.contains(&t) {
                candidates.push(t);
            }
        }

        // 6. Consonant swap variations — try swapping each ambiguous consonant one at a time
        let swaps = self.generate_consonant_swaps(&word);
        for swap in swaps {
            if !candidates.contains(&swap) {
                candidates.push(swap);
            }
        }

        // 7. Rank candidates by: user prefs > dictionary > frequency > unknown
        self.rank_candidates(&word, &mut candidates, is_dict_word, prefs);

        // Cap at 8 candidates
        candidates.truncate(8);
        candidates
    }

    /// Rank candidates using user preferences, dictionary presence, and word frequency.
    fn rank_candidates(
        &self,
        input: &str,
        candidates: &mut Vec<String>,
        is_dict_word: bool,
        prefs: Option<&UserPreferences>,
    ) {
        let dict_entries: Vec<String> = self.dictionary
            .get(input)
            .cloned()
            .unwrap_or_default();

        candidates.sort_by(|a, b| {
            let score_a = self.candidate_score(input, a, is_dict_word, &dict_entries, prefs);
            let score_b = self.candidate_score(input, b, is_dict_word, &dict_entries, prefs);
            // Higher score = better, so reverse order
            score_b.cmp(&score_a)
        });
    }

    /// Score a candidate. Higher = better.
    fn candidate_score(
        &self,
        input: &str,
        candidate: &str,
        _is_dict_word: bool,
        dict_entries: &[String],
        prefs: Option<&UserPreferences>,
    ) -> u64 {
        let mut score: u64 = 0;

        // User preference: 1M points per selection count (strongest signal)
        if let Some(prefs) = prefs {
            let user_score = prefs.score(input, candidate);
            score += user_score as u64 * 1_000_000;
        }

        // Dictionary match: 100k bonus (curated entries are high quality)
        if dict_entries.contains(&candidate.to_string()) {
            score += 100_000;
            // First dict entry gets extra bonus (it's the preferred form)
            if dict_entries.first().map(|s| s.as_str()) == Some(candidate) {
                score += 50_000;
            }
        }

        // Frequency list: up to 300k points (real words beat gibberish)
        // Words with individual Arabic tokens all in the frequency list score well.
        // For single-word candidates, check directly.
        // Score inversely proportional to rank (rank 0 = most common = highest score).
        let words: Vec<&str> = candidate.split_whitespace().collect();
        let mut freq_score: u64 = 0;
        let mut all_found = true;
        for w in &words {
            if let Some(rank) = self.frequency.rank(w) {
                // Inverse rank: lower rank = higher score
                freq_score += 300_000u64.saturating_sub(rank as u64);
            } else {
                all_found = false;
            }
        }
        if all_found && !words.is_empty() {
            score += freq_score / words.len() as u64;
        }

        score
    }

    /// Generate tanween fatha variants for candidates ending in ان.
    /// In Arabic, words ending in "-an" in Arabizi often represent tanween fatha (ً)
    /// rather than a literal alef-noon. E.g. "sahlan" → سهلاً not just سهلان.
    fn generate_tanween_variants(candidates: &[String]) -> Vec<String> {
        let mut variants = Vec::new();
        for candidate in candidates {
            if candidate.ends_with("ان") {
                let base = &candidate[..candidate.len() - "ان".len()];
                variants.push(format!("{}اً", base));
            } else if candidate.ends_with("ن") && !candidate.ends_with("ان") {
                // Cases where vowel was dropped: e.g. سهلن → سهلاً
                let base = &candidate[..candidate.len() - "ن".len()];
                variants.push(format!("{}اً", base));
            }
        }
        variants
    }

    /// Generate variations by swapping ambiguous consonants.
    /// Uses a substitution map to override specific positions, keeping full-word context
    /// intact (so taa marbuta and other context-sensitive rules work correctly).
    ///
    /// Two strategies:
    /// 1. Swap one instance at a time (e.g. لذيز for "lazeez")
    /// 2. Swap ALL instances of the same pattern at once (e.g. لذيذ for "lazeez")
    fn generate_consonant_swaps(&self, word: &str) -> Vec<String> {
        let chars: Vec<char> = word.chars().collect();
        let len = chars.len();
        let mut results = Vec::new();

        // First, find all ambiguous positions: (position, pattern, alternatives)
        let mut ambiguous_positions: Vec<(usize, &str, &[&str])> = Vec::new();
        let mut pos = 0;
        while pos < len {
            // Skip digraph separator
            if chars[pos] == '-' {
                pos += 1;
                continue;
            }
            let has_separator = pos + 1 < len && chars[pos + 1] == '-';
            let remaining: String = chars[pos..].iter().collect();
            let mut found = false;
            for (pattern, _primary, alternatives) in AMBIGUOUS_CONSONANTS {
                if has_separator && pattern.len() > 1 {
                    continue; // separator breaks this digraph
                }
                if remaining.starts_with(pattern) {
                    ambiguous_positions.push((pos, pattern, alternatives));
                    pos += pattern.len();
                    found = true;
                    break;
                }
            }
            if !found {
                let max_len = if has_separator { 1 } else { usize::MAX };
                if let Some((pattern, _)) = Self::match_consonant_max(&remaining, max_len) {
                    pos += pattern.len();
                } else if let Some((pattern, _)) = Self::match_long_vowel(&remaining) {
                    pos += pattern.len();
                } else {
                    pos += 1;
                }
            }
        }

        // Strategy 1: Swap one at a time
        for (i, &(_swap_pos, _pattern, alternatives)) in ambiguous_positions.iter().enumerate() {
            for alt in alternatives {
                let mut overrides = HashMap::new();
                overrides.insert(i, *alt);
                let swapped = self.transliterate_with_overrides(word, &ambiguous_positions, &overrides);
                if !results.contains(&swapped) {
                    results.push(swapped);
                }
            }
        }

        // Strategy 2: Swap ALL instances of the same pattern to the same alternative.
        // E.g. "lazeez" has two "z" → swap both to ذ to get لذيذ.
        let mut patterns_seen: Vec<&str> = Vec::new();
        for &(_, pattern, _) in &ambiguous_positions {
            if !patterns_seen.contains(&pattern) {
                patterns_seen.push(pattern);
            }
        }

        for target_pattern in &patterns_seen {
            let matching_indices: Vec<usize> = ambiguous_positions
                .iter()
                .enumerate()
                .filter(|(_, (_, p, _))| *p == *target_pattern)
                .map(|(i, _)| i)
                .collect();

            if matching_indices.len() < 2 {
                continue; // Single instance already handled in strategy 1
            }

            let alternatives = ambiguous_positions[matching_indices[0]].2;
            for alt in alternatives {
                let mut overrides = HashMap::new();
                for &idx in &matching_indices {
                    overrides.insert(idx, *alt);
                }
                let swapped = self.transliterate_with_overrides(word, &ambiguous_positions, &overrides);
                if !results.contains(&swapped) {
                    results.push(swapped);
                }
            }
        }

        results
    }

    /// Transliterate a full word but override specific ambiguous consonants.
    /// This preserves full-word context for taa marbuta and vowel handling.
    fn transliterate_with_overrides(
        &self,
        word: &str,
        ambiguous: &[(usize, &str, &[&str])],
        overrides: &HashMap<usize, &str>,
    ) -> String {
        let chars: Vec<char> = word.chars().collect();
        let len = chars.len();
        let mut result = String::new();
        let mut pos = 0;

        while pos < len {
            // Check if this position is an overridden ambiguous consonant
            let override_match = ambiguous.iter().enumerate().find(|(_, (p, _, _))| *p == pos);
            if let Some((idx, (_, pattern, _))) = override_match {
                if let Some(alt) = overrides.get(&idx) {
                    result.push_str(alt);
                    pos += pattern.len();
                    continue;
                }
            }

            // Context-aware hamza placement for '2'
            if chars[pos] == '2' {
                result.push_str(Self::resolve_hamza(&chars, pos));
                pos += 1;
                continue;
            }

            // Digraph separator
            let has_separator = pos + 1 < len && chars[pos + 1] == '-';
            let remaining: String = chars[pos..].iter().collect();

            // Consonant mappings (limited to 1 char if separator follows)
            let max_len = if has_separator { 1 } else { usize::MAX };
            if let Some((pattern, arabic)) = Self::match_consonant_max(&remaining, max_len) {
                result.push_str(arabic);
                pos += pattern.len();
                if has_separator && pos < len && chars[pos] == '-' {
                    pos += 1;
                }
                continue;
            }

            // Skip bare separator
            if chars[pos] == '-' {
                pos += 1;
                continue;
            }

            // Long vowel mappings
            if let Some((pattern, arabic)) = Self::match_long_vowel(&remaining) {
                result.push_str(arabic);
                pos += pattern.len();
                continue;
            }

            // Short vowels — same context-aware logic as rule_based_transliterate
            if is_vowel_char(chars[pos]) {
                let at_start = pos == 0;
                let at_end = pos == len - 1;
                let is_taa_marbuta = at_end
                    && (chars[pos] == 'a' || chars[pos] == 'e')
                    && pos > 0
                    && !is_vowel_char(chars[pos - 1]);

                if at_start {
                    Self::push_initial_vowel(&mut result, chars[pos]);
                } else if is_taa_marbuta {
                    result.push_str("ة");
                }
                // Use DropMiddle-like behavior: drop middle vowels
                pos += 1;
                continue;
            }

            result.push(chars[pos]);
            pos += 1;
        }

        result
    }

    /// Context-aware rule-based transliteration with configurable vowel handling.
    fn rule_based_transliterate(&self, word: &str, vowel_mode: VowelMode) -> String {
        let chars: Vec<char> = word.chars().collect();
        let len = chars.len();
        let mut result = String::new();
        let mut pos = 0;

        while pos < len {
            // 0. Context-aware hamza placement for '2'
            if chars[pos] == '2' {
                result.push_str(Self::resolve_hamza(&chars, pos));
                pos += 1;
                continue;
            }

            // Digraph separator: '-' limits the next match to single characters
            let has_separator = pos + 1 < len && chars[pos + 1] == '-';
            let remaining: String = chars[pos..].iter().collect();

            // 1. Consonant mappings (limited to 1 char if separator follows)
            let max_len = if has_separator { 1 } else { usize::MAX };
            if let Some((pattern, arabic)) = Self::match_consonant_max(&remaining, max_len) {
                result.push_str(arabic);
                pos += pattern.len();
                // Skip the separator
                if has_separator && pos < len && chars[pos] == '-' {
                    pos += 1;
                }
                continue;
            }

            // Skip bare separator (e.g. between vowels)
            if chars[pos] == '-' {
                pos += 1;
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

                // Word-final 'a' or 'e' after a consonant → taa marbuta (ة)
                // This is the most common Arabic feminine ending
                let is_taa_marbuta = at_end
                    && (chars[pos] == 'a' || chars[pos] == 'e')
                    && pos > 0
                    && !is_vowel_char(chars[pos - 1]);

                match vowel_mode {
                    VowelMode::KeepAll => {
                        if at_start {
                            Self::push_initial_vowel(&mut result, chars[pos]);
                        } else if is_taa_marbuta {
                            result.push_str("ة");
                        } else if let Some((_, arabic)) = Self::match_short_vowel(&remaining) {
                            result.push_str(arabic);
                        }
                    }
                    VowelMode::DropAll => {
                        if at_start {
                            Self::push_initial_vowel(&mut result, chars[pos]);
                        } else if is_taa_marbuta {
                            result.push_str("ة");
                        }
                        // Otherwise drop
                    }
                    VowelMode::DropMiddle => {
                        if at_start {
                            Self::push_initial_vowel(&mut result, chars[pos]);
                        } else if is_taa_marbuta {
                            result.push_str("ة");
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

    /// Resolve hamza (2) to the appropriate Arabic form based on surrounding vowel context.
    /// Arabic hamza sits on different "seats" depending on the strongest adjacent vowel:
    /// - kasra (i/e) → ئ (on ya) — strongest
    /// - damma (u/o) → ؤ (on waw)
    /// - fatha (a) or no vowel → أ (on alef) — default for mid-word
    /// - word-final after consonant → ء (standalone)
    fn resolve_hamza(chars: &[char], pos: usize) -> &'static str {
        let len = chars.len();
        let at_start = pos == 0;
        let at_end = pos == len - 1;

        if at_start {
            return match chars.get(pos + 1) {
                Some(&'i') | Some(&'e') => "إ",
                _ => "أ",
            };
        }

        if at_end {
            return "ء";
        }

        // Mid-word: strongest adjacent vowel determines the seat
        let prev_strength = chars.get(pos.wrapping_sub(1)).and_then(|&c| Self::vowel_strength(c));
        let next_strength = chars.get(pos + 1).and_then(|&c| Self::vowel_strength(c));

        let strength = match (prev_strength, next_strength) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };

        match strength {
            Some(3) => "ئ",  // kasra → hamza on ya
            Some(2) => "ؤ",  // damma → hamza on waw
            _ => "أ",        // fatha, no vowel → hamza on alef (most common)
        }
    }

    fn vowel_strength(c: char) -> Option<u8> {
        match c {
            'i' | 'e' => Some(3),
            'u' | 'o' => Some(2),
            'a' => Some(1),
            _ => None,
        }
    }

    fn push_initial_vowel(result: &mut String, c: char) {
        match c {
            'i' => result.push_str("إ"),
            'u' => result.push_str("أ"),
            'o' => result.push_str("أ"),
            _ => result.push_str("ا"),
        }
    }

    /// Match a consonant pattern, with optional max length limit.
    /// When `max_len` is 1, digraphs/trigraphs are skipped — used for separator-broken digraphs.
    fn match_consonant_max(input: &str, max_len: usize) -> Option<(&'static str, &'static str)> {
        for (pattern, arabic) in CONSONANT_MAPPINGS {
            if pattern.len() <= max_len && input.starts_with(pattern) {
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
        assert!(results.contains(&"شكرًا".to_string()), "Expected شكرًا in {:?}", results);
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

    #[test]
    fn taa_marbuta_at_word_end() {
        let e = engine();
        // "gameela" should produce جميلة with taa marbuta, not جميلا
        let results = e.transliterate_word("gameela");
        assert!(results.iter().any(|r| r.contains("ة")),
            "Expected ة (taa marbuta) in candidates for 'gameela', got {:?}", results);
    }

    #[test]
    fn taa_marbuta_not_after_vowel() {
        let e = engine();
        // "yalla" ends in 'a' but after another 'a' (vowel), not a consonant
        // so it should NOT get taa marbuta
        let results = e.transliterate_word("aa");
        assert!(!results.iter().any(|r| r.contains("ة")),
            "Should not have ة for 'aa', got {:?}", results);
    }

    #[test]
    fn hamza_mid_word_on_alef() {
        let e = engine();
        // "bd2t" → بدأت (hamza on alef, no adjacent vowels → default alef)
        let results = e.transliterate_word("bd2t");
        assert!(results.iter().any(|r| r.contains("أ")),
            "Expected أ (hamza on alef) for 'bd2t', got {:?}", results);
    }

    #[test]
    fn hamza_with_damma_on_waw() {
        let e = engine();
        // "su2al" → سؤال (hamza on waw because of adjacent u)
        let results = e.transliterate_word("su2al");
        assert!(results.iter().any(|r| r.contains("ؤ")),
            "Expected ؤ (hamza on waw) for 'su2al', got {:?}", results);
    }

    #[test]
    fn hamza_with_kasra_on_ya() {
        let e = engine();
        // "ra2is" → رئيس (hamza on ya because of adjacent i)
        let results = e.transliterate_word("ra2is");
        assert!(results.iter().any(|r| r.contains("ئ")),
            "Expected ئ (hamza on ya) for 'ra2is', got {:?}", results);
    }

    #[test]
    fn hamza_word_final_standalone() {
        let e = engine();
        // "masa2" → مساء (standalone hamza at end)
        let results = e.transliterate_word("masa2");
        assert!(results.iter().any(|r| r.contains("ء")),
            "Expected ء (standalone hamza) for 'masa2', got {:?}", results);
    }

    #[test]
    fn hamza_word_initial() {
        let e = engine();
        // "2amal" → أمل (hamza on alef at start)
        let results = e.transliterate_word("2amal");
        assert!(results.iter().any(|r| r.starts_with("أ")),
            "Expected أ (hamza on alef) at start for '2amal', got {:?}", results);
    }
}
