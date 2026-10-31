use std::collections::HashMap;

use crate::cldr_ar::CLDR_AR;
use crate::dictionary::build_dictionary;
use crate::emoji::{build_emoji_map, build_emoticon_map};
use crate::frequency::FrequencyList;
use crate::lattice;
use crate::tashkeela::TashkeelaList;
use crate::mappings::{
    is_vowel_char, AMBIGUOUS_CONSONANTS, CONSONANT_MAPPINGS, LONG_VOWEL_MAPPINGS,
    PICKER_ALTERNATIVES, SHORT_VOWELS,
};
use crate::user_prefs::UserPreferences;

/// Paths kept when enumerating the candidate lattice.
const LATTICE_BEAM: usize = 1500;
/// Weight of the language model (log10 word count) against channel cost.
/// Weight of the language model (log10 word count) against channel cost.
const LM_WEIGHT: f64 = 0.7;
/// Discount for words attested only in the (MSA-heavy) Tashkeela corpus.
const LM_TASHKEELA_PENALTY: f64 = 2000.0;
/// Score of a word attested in neither corpus.
const LM_UNKNOWN: f64 = -3000.0;
/// Weight of the colloquial word list, added on top of the main LM score.
/// Tuned on held-out benchmark words (top-1 78% → 82%).
const DIALECT_WEIGHT: f64 = 0.35;
/// Penalty per clear vowel clash with a candidate's corpus vocalization
/// (half for a soft clash), capped at two clashes.
const VOWEL_MISMATCH_PENALTY: i64 = 1000;
/// Discount for words attested only in the (MSA-heavy) Tashkeela corpus.

/// Score of a word attested in neither corpus.

/// Penalty when a candidate's corpus vocalization contradicts typed vowels.


/// The main transliteration engine.
///
/// Candidate generation (see `lattice.rs`): the Latin word is split into
/// segments, each with weighted Arabic readings — consonant swaps (s→ص),
/// short vs. long vowels, doubled consonant → shadda, hamza seats and
/// Egyptian 2→ق, taa marbuta / alef / alef maqsura / ه endings, tanween.
/// The cheapest combinations are enumerated, plus dictionary entries.
///
/// Ranking (noisy channel):
/// 1. User preference (previously selected candidates for this input)
/// 2. Curated dictionary entries
/// 3. log-frequency (OpenSubtitles, Tashkeela fallback) − channel cost −
///    vowel-agreement penalty against the corpus vocalization
pub struct TransliterationEngine {
    dictionary: HashMap<String, Vec<String>>,
    emoji_map: HashMap<String, Vec<&'static str>>,
    emoticon_map: HashMap<&'static str, Vec<&'static str>>,
    cldr_index: HashMap<String, Vec<&'static str>>,
    frequency: FrequencyList,
    dialect: FrequencyList,
    tashkeela: TashkeelaList,
}

impl TransliterationEngine {
    pub fn new() -> Self {
        Self {
            dictionary: build_dictionary(),
            emoji_map: build_emoji_map(),
            emoticon_map: build_emoticon_map(),
            cldr_index: Self::build_cldr_index(),
            frequency: FrequencyList::load(),
            dialect: FrequencyList::load_dialect(),
            tashkeela: TashkeelaList::load(),
        }
    }

    /// Return the corpus's most-frequent fully-vocalized form for a given
    /// (already transliterated) Arabic candidate, if one exists. Used by the
    /// tashkeel modifier (Cmd+T) to offer a one-shot diacritized form at the
    /// word level — *not* applied to default candidates, which stay bare.
    pub fn lookup_vocalized(&self, candidate: &str) -> Option<String> {
        self.tashkeela.vocalized(candidate).map(|s| s.to_string())
    }

    fn build_cldr_index() -> HashMap<String, Vec<&'static str>> {
        let mut index: HashMap<String, Vec<&'static str>> = HashMap::new();
        for (emoji, keywords) in CLDR_AR {
            for kw in *keywords {
                index.entry(kw.to_string()).or_default().push(emoji);
            }
        }
        index
    }

    /// Look up emojis for Arabic candidates.
    ///
    /// Priority:
    /// 1. Handwritten map — curated, exact matches (highest quality)
    /// 2. CLDR reverse index — broad coverage from Unicode keyword data
    ///
    /// Returns up to 3 emojis, deduplicated.
    pub fn lookup_emojis(&self, candidates: &[String]) -> Vec<String> {
        // 1. Handwritten map wins if any candidate matches
        for candidate in candidates {
            if let Some(emojis) = self.emoji_map.get(candidate) {
                return emojis.iter().map(|e| e.to_string()).collect();
            }
        }

        // 2. CLDR reverse index — collect from best candidates first
        let mut results: Vec<String> = Vec::new();
        'outer: for candidate in candidates.iter().take(3) {
            if let Some(emojis) = self.cldr_index.get(candidate.as_str()) {
                for &emoji in emojis {
                    let s = emoji.to_string();
                    if !results.contains(&s) {
                        results.push(s);
                    }
                    if results.len() >= 3 {
                        break 'outer;
                    }
                }
            }
        }
        results
    }

    /// Check if input is a text emoticon. Input must already be lowercased.
    /// Returns emoji candidates if matched, or None to fall through to normal transliteration.
    pub fn lookup_emoticon(&self, input: &str) -> Option<Vec<String>> {
        self.emoticon_map
            .get(input)
            .map(|emojis| emojis.iter().map(|e| e.to_string()).collect())
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

    /// Scan a word and return one slot per recognized Latin letter token —
    /// consonants (incl. digraphs), vowels, and number-letters. Non-overridable
    /// tokens (like 'b' or '7') still get a slot with an empty `alternatives`
    /// list so the picker can still target them and show "no swap available".
    /// Slots are in left-to-right order, indexed by their starting char position
    /// in the lowercased input.
    pub fn letter_slots(&self, word: &str) -> Vec<LetterSlot> {
        let word = word.to_lowercase();
        let chars: Vec<char> = word.chars().collect();
        let len = chars.len();
        let mut slots = Vec::new();
        let mut pos = 0;

        while pos < len {
            if chars[pos] == '-' {
                pos += 1;
                continue;
            }
            let has_separator = pos + 1 < len && chars[pos + 1] == '-';
            let remaining: String = chars[pos..].iter().collect();

            // 1. Try the wider picker table first (covers consonants + vowels + 2).
            // Always prefer the longest match: entries such as `t` and `th` both
            // share a prefix, and table order must not change how the word is
            // tokenized.
            let max_len = if has_separator { 1 } else { usize::MAX };
            if let Some((pattern, primary, alternatives)) =
                Self::match_picker_max(&remaining, max_len)
            {
                slots.push(LetterSlot {
                    pos,
                    pattern: pattern.to_string(),
                    primary: primary.to_string(),
                    alternatives: alternatives.iter().map(|s| (*s).to_string()).collect(),
                });
                pos += pattern.len();
                if has_separator && pos < len && chars[pos] == '-' {
                    pos += 1;
                }
                continue;
            }

            // 2. Fall back to consonant table — token has no alternatives.
            if let Some((pattern, primary)) = Self::match_consonant_max(&remaining, max_len) {
                slots.push(LetterSlot {
                    pos,
                    pattern: pattern.to_string(),
                    primary: primary.to_string(),
                    alternatives: Vec::new(),
                });
                pos += pattern.len();
                if has_separator && pos < len && chars[pos] == '-' {
                    pos += 1;
                }
                continue;
            }

            // 3. Long vowel without an alternative (rare given step 1).
            if let Some((pattern, primary)) = Self::match_long_vowel(&remaining) {
                slots.push(LetterSlot {
                    pos,
                    pattern: pattern.to_string(),
                    primary: primary.to_string(),
                    alternatives: Vec::new(),
                });
                pos += pattern.len();
                continue;
            }

            // 4. Unmapped char — skip without a slot.
            pos += 1;
        }
        slots
    }

    /// Transliterate a single word, applying per-position Arabic-letter overrides
    /// (keyed by Latin char position from `letter_slots`). The override-derived
    /// candidate is pinned to the top of the returned list.
    ///
    /// Overrides may target any letter token (consonant, vowel, or hamza) — not
    /// just the auto-suggester's ambiguous-consonant set.
    pub fn transliterate_word_with_overrides(
        &self,
        word: &str,
        overrides_by_pos: &HashMap<usize, String>,
        prefs: Option<&UserPreferences>,
    ) -> Vec<String> {
        let word_lower = word.to_lowercase();
        let mut candidates = self.transliterate_word_ranked(&word_lower, prefs);

        if overrides_by_pos.is_empty() {
            return candidates;
        }

        // Validate overrides against the slot table — drop any whose value isn't
        // a legal alternative (or the primary) for the slot at that position.
        // A combined picker chip may collapse several Latin slots into one
        // Arabic mapping (e.g. s+h -> ش). In that encoding the first slot holds
        // the Arabic result and every consumed following slot holds an empty
        // string; validate that group as a span rather than rejecting the first
        // value as an invalid replacement for `s` by itself.
        // Picking the primary is *not* a no-op: for vowels in middle position the
        // rule-based path would drop the letter, and the user's explicit pick
        // must force it to appear.
        //
        // A choice is also accepted when it equals primary/alt followed by one or
        // more Arabic combining marks (tashkeel): the user can pin "بَ" at a slot
        // whose primary is "ب". We pass the *original* override (with tashkeel)
        // into the override-aware path so the marks survive to the output.
        let slots = self.letter_slots(&word_lower);
        let mut valid: HashMap<usize, String> = HashMap::new();
        for slot in &slots {
            if let Some(choice) = overrides_by_pos.get(&slot.pos) {
                if choice.is_empty() {
                    // Empty values are admitted only as part of a validated
                    // collapsed span, when processing its non-empty anchor.
                    continue;
                }

                if let Some(span_positions) = Self::validated_collapsed_span(
                    &word_lower,
                    slot,
                    &slots,
                    choice,
                    overrides_by_pos,
                ) {
                    valid.insert(slot.pos, choice.clone());
                    for consumed_pos in span_positions.into_iter().skip(1) {
                        valid.insert(consumed_pos, String::new());
                    }
                    continue;
                }

                let base = strip_tashkeel(choice);
                let base_matches_primary = base == slot.primary;
                let base_matches_alt = slot.alternatives.iter().any(|a| *a == base);
                if base_matches_primary || base_matches_alt {
                    valid.insert(slot.pos, choice.clone());
                }
            }
        }

        if !valid.is_empty() {
            let valid_refs: HashMap<usize, &str> =
                valid.iter().map(|(k, v)| (*k, v.as_str())).collect();
            let pinned = self.rule_based_with_overrides(&word_lower, &valid_refs);
            candidates.retain(|c| c != &pinned);
            candidates.insert(0, pinned);
            candidates.truncate(8);
        }

        candidates
    }

    /// Validate the UI's compact representation of a multi-Latin-letter
    /// override. Returns the participating slot positions when `choice` is a
    /// legal primary/alternative Arabic mapping for their combined pattern and
    /// all slots after the anchor are explicitly marked as consumed (`""`).
    fn validated_collapsed_span(
        word: &str,
        anchor: &LetterSlot,
        slots: &[LetterSlot],
        choice: &str,
        overrides: &HashMap<usize, String>,
    ) -> Option<Vec<usize>> {
        let chars: Vec<char> = word.chars().collect();
        let remaining: String = chars.get(anchor.pos..)?.iter().collect();
        let base_choice = strip_tashkeel(choice);

        for (pattern, primary) in CONSONANT_MAPPINGS {
            if pattern.len() <= anchor.pattern.len() || !remaining.starts_with(pattern) {
                continue;
            }

            let matches_primary = base_choice == *primary;
            let matches_alternative = AMBIGUOUS_CONSONANTS
                .iter()
                .find(|(ambiguous_pattern, _, _)| ambiguous_pattern == pattern)
                .map(|(_, _, alternatives)| alternatives.iter().any(|alt| *alt == base_choice))
                .unwrap_or(false);
            if !matches_primary && !matches_alternative {
                continue;
            }

            let pattern_end = anchor.pos + pattern.chars().count();
            let span_slots: Vec<&LetterSlot> = slots
                .iter()
                .filter(|slot| slot.pos >= anchor.pos && slot.pos < pattern_end)
                .collect();
            let combined_pattern: String = span_slots
                .iter()
                .map(|slot| slot.pattern.as_str())
                .collect();
            if span_slots.len() < 2 || combined_pattern != *pattern {
                continue;
            }
            if span_slots
                .iter()
                .skip(1)
                .all(|slot| overrides.get(&slot.pos).is_some_and(String::is_empty))
            {
                return Some(span_slots.iter().map(|slot| slot.pos).collect());
            }
        }

        None
    }

    /// Like `rule_based_transliterate(DropMiddle)` but emits an override letter
    /// directly when a Latin position has one. Picker overrides bypass the
    /// vowel-drop and taa-marbuta context rules — the user's choice always wins.
    fn rule_based_with_overrides(
        &self,
        word: &str,
        overrides: &HashMap<usize, &str>,
    ) -> String {
        let chars: Vec<char> = word.chars().collect();
        let len = chars.len();
        let mut result = String::new();
        let mut pos = 0;

        while pos < len {
            // Override at this position wins. Advance by the matched pattern length.
            if let Some(arabic) = overrides.get(&pos) {
                let remaining: String = chars[pos..].iter().collect();
                let advance = Self::match_picker_max(&remaining, usize::MAX)
                    .map(|(pattern, _, _)| pattern.len())
                    .unwrap_or(1);
                result.push_str(arabic);
                pos += advance;
                continue;
            }

            // No override — fall through to the normal DropMiddle behavior.
            if chars[pos] == '2' {
                result.push_str(Self::resolve_hamza(&chars, pos));
                pos += 1;
                continue;
            }
            let has_separator = pos + 1 < len && chars[pos + 1] == '-';
            let remaining: String = chars[pos..].iter().collect();
            let max_len = if has_separator { 1 } else { usize::MAX };
            if let Some((pattern, arabic)) = Self::match_consonant_max(&remaining, max_len) {
                result.push_str(arabic);
                pos += pattern.len();
                if has_separator && pos < len && chars[pos] == '-' {
                    pos += 1;
                }
                continue;
            }
            if chars[pos] == '-' {
                pos += 1;
                continue;
            }
            if let Some((pattern, arabic)) = Self::match_long_vowel(&remaining) {
                result.push_str(arabic);
                pos += pattern.len();
                continue;
            }
            if is_vowel_char(chars[pos]) {
                let at_start = pos == 0;
                let at_end = pos == len - 1;
                let before_final = pos + 1 == len - 1;
                let is_taa_marbuta = at_end
                    && (chars[pos] == 'a' || chars[pos] == 'e')
                    && pos > 0
                    && !is_vowel_char(chars[pos - 1]);
                if at_start {
                    Self::push_initial_vowel(&mut result, chars[pos]);
                } else if is_taa_marbuta {
                    result.push_str("ة");
                } else if at_end || before_final {
                    if let Some((_, arabic)) = Self::match_short_vowel(&remaining) {
                        result.push_str(arabic);
                    }
                }
                pos += 1;
                continue;
            }
            result.push(chars[pos]);
            pos += 1;
        }

        result
    }

    /// Convert a string of ASCII digits (with optional `.` / `,`) to Arabic-Indic
    /// digits. Returns `None` if the token contains any non-digit / non-punct
    /// char, or if it's a single bare digit (which in Arabizi usually stands
    /// for a consonant — "7" → ح, "3" → ع — and is handled by the rule-based
    /// path). Multi-digit tokens like "2026" / "100.5" almost always mean the
    /// number itself.
    fn try_digit_token(word: &str) -> Option<String> {
        let mut digit_count: usize = 0;
        let mut out = String::with_capacity(word.len() * 2);
        for c in word.chars() {
            match c {
                '0' => { out.push('٠'); digit_count += 1; }
                '1' => { out.push('١'); digit_count += 1; }
                '2' => { out.push('٢'); digit_count += 1; }
                '3' => { out.push('٣'); digit_count += 1; }
                '4' => { out.push('٤'); digit_count += 1; }
                '5' => { out.push('٥'); digit_count += 1; }
                '6' => { out.push('٦'); digit_count += 1; }
                '7' => { out.push('٧'); digit_count += 1; }
                '8' => { out.push('٨'); digit_count += 1; }
                '9' => { out.push('٩'); digit_count += 1; }
                '.' | ',' => out.push(c),
                _ => return None,
            }
        }
        // A token made entirely of digits / . / , is always a number. Inside a
        // word (e.g. "7abibi"), digits stay as Arabizi consonants — that path
        // never reaches here because the non-digit char makes us return None
        // above.
        if digit_count >= 1 { Some(out) } else { None }
    }

    /// Transliterate a single word with optional user preference ranking.
    pub fn transliterate_word_ranked(&self, word: &str, prefs: Option<&UserPreferences>) -> Vec<String> {
        let word = word.to_lowercase();

        // Pure-numeric tokens like "2026" / "100.5" should convert to Arabic-Indic
        // digits, not be read as consonants (2→ء, 7→ح, etc.). The picker and the
        // Tab-to-keep-Latin escape both still work because the Latin form is
        // committed verbatim when the user presses Tab.
        if let Some(indic) = Self::try_digit_token(&word) {
            return vec![indic];
        }

        // (candidate, channel cost). Cost 0 = canonical reading of the Latin.
        let mut candidates: Vec<(String, i32)> = Vec::new();
        let mut index: HashMap<String, usize> = HashMap::new();
        let mut push = |s: String, cost: i32| match index.get(&s) {
            Some(&i) => candidates[i].1 = candidates[i].1.min(cost),
            None => {
                index.insert(s.clone(), candidates.len());
                candidates.push((s, cost));
            }
        };

        // 1. Dictionary entries (curated, always win the ranking).
        if let Some(dict_entries) = self.dictionary.get(&word) {
            for entry in dict_entries {
                push(entry.clone(), 0);
            }
        }

        // 2. Weighted lattice: every combination of per-segment readings
        // (consonant swaps, short/long vowels, shadda, hamza seats, taa
        // marbuta / alef / alef maqsura endings, tanween), cheapest first.
        let segments = lattice::build(&word);
        // Very long tokens (pasted text, URLs) get a narrower beam so the
        // search stays well under a millisecond-scale budget.
        let beam = if segments.len() > 14 { LATTICE_BEAM / 5 } else { LATTICE_BEAM };
        for (s, cost) in lattice::enumerate(&segments, beam) {
            push(s, cost);
        }

        // 3. Inject any previously-confirmed candidates the user has selected
        // for this input. These wouldn't otherwise be generated when they
        // require multi-letter combinations the lattice never emits
        // (e.g. picker-derived "بالظبط" from "baalzbt").
        if let Some(prefs) = prefs {
            for known in prefs.known_candidates(&word) {
                push(known, 0);
            }
        }

        // 4. Rank: user prefs > dictionary > language model − channel cost.
        self.rank_candidates(&word, &mut candidates, prefs);

        candidates.into_iter().take(8).map(|(s, _)| s).collect()
    }

    /// Rank candidates using user preferences, dictionary presence, word
    /// frequency and the lattice's channel cost.
    fn rank_candidates(
        &self,
        input: &str,
        candidates: &mut Vec<(String, i32)>,
        prefs: Option<&UserPreferences>,
    ) {
        let dict_entries: Vec<String> = self.dictionary
            .get(input)
            .cloned()
            .unwrap_or_default();

        let mut scored: Vec<(i64, i32, usize)> = candidates
            .iter()
            .enumerate()
            .map(|(i, (c, cost))| (self.candidate_score(input, c, *cost, &dict_entries, prefs), *cost, i))
            .collect();
        // Higher score first; ties → cheaper reading → generation order.
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        let old = std::mem::take(candidates);
        *candidates = scored.into_iter().map(|(_, _, i)| old[i].clone()).collect();
    }

    /// Score a candidate. Higher = better.
    fn candidate_score(
        &self,
        input: &str,
        candidate: &str,
        cost: i32,
        dict_entries: &[String],
        prefs: Option<&UserPreferences>,
    ) -> i64 {
        let mut score: i64 = 0;

        // User preference: strongest signal, per selection count.
        if let Some(prefs) = prefs {
            score += prefs.score(input, candidate) as i64 * 1_000_000_000;
        }

        // Dictionary match: curated entries outrank anything generated.
        if let Some(idx) = dict_entries.iter().position(|d| d == candidate) {
            score += 100_000_000;
            // Earlier entries are the preferred forms.
            score += 50_000_000 / (idx as i64 + 1);
        }

        score + self.lm_score(candidate) + self.dialect_score(candidate) - cost as i64
            - self.vowel_penalty(input, candidate)
    }

    /// Penalty when the corpus vocalization of `candidate` contradicts the
    /// vowels the user typed. Frequent MSA words otherwise steal the top slot
    /// from the intended word with different vowels: `3amel` (عَامِل) losing
    /// to عَمَل, `katab` (كَتَب) to كِتَاب, `el2amar` (القَمَر) to الأَمْر.
    ///
    /// Compares the typed vowel sequence (a / i-e / u-o, long vowels collapsed)
    /// against the harakat of the Tashkeela form. Only contradictions count:
    /// a differing vowel, or more typed vowels than the word has. Typing
    /// *fewer* vowels is normal in Arabizi (`b7bk`, `ktir`) and is free.
    /// Words without a vocalized form (most dialect words) are never penalized.
    fn vowel_penalty(&self, input: &str, candidate: &str) -> i64 {
        if candidate.contains(' ') {
            return 0;
        }
        let variants = self.tashkeela.variants(&strip_tashkeel(candidate));
        if variants.is_empty() {
            return 0;
        }

        let latin: Vec<char> = input.chars().filter(|c| *c != '-').collect();
        let starts_with_vowel = latin.first().is_some_and(|c| is_vowel_char(*c));
        let ends_with_vowel = latin.last().is_some_and(|c| is_vowel_char(*c));

        // Typed vowel classes; a run of vowels is one vowel (aa, ee, ou, ei…).
        let mut typed: Vec<char> = Vec::new();
        let mut i = 0;
        while i < latin.len() {
            if !is_vowel_char(latin[i]) {
                i += 1;
                continue;
            }
            let start = i;
            while i < latin.len() && is_vowel_char(latin[i]) {
                i += 1;
            }
            let run: String = latin[start..i].iter().collect();
            // a: fatha · i: kasra (also long ee/ii) · e: Egyptian e, which
            // writes kasra *or* a reduced fatha · u: damma · y / w: diphthongs.
            let class = match run.as_str() {
                "ei" | "ai" | "ay" => 'y',
                "au" | "aw" => 'w',
                "e" => 'e',
                r if r.starts_with('a') => 'a',
                r if r.starts_with('e') || r.starts_with('i') => 'i',
                _ => 'u',
            };
            typed.push(class);
        }
        if starts_with_vowel && !typed.is_empty() {
            typed.remove(0);
        }

        // A word can be read several ways (أَمْر noun / أَمَرَ verb). Weight
        // each reading's clash by how often the corpus uses it, so a word is
        // only spared when a common reading fits what was typed.
        let total: u64 = variants.iter().map(|(_, c)| *c as u64).sum::<u64>().max(1);
        let weighted: u64 = variants
            .iter()
            .map(|(v, c)| Self::vowel_clash(&typed, v, starts_with_vowel, ends_with_vowel) as u64 * *c as u64)
            .sum();
        (VOWEL_MISMATCH_PENALTY as i128 * weighted as i128 / (2 * total as i128)) as i64
    }

    /// Contradiction between typed vowel classes and one vocalized form, in
    /// half-points (0 = fits, capped at 4).
    fn vowel_clash(typed: &[char], vocalized: &str, starts_with_vowel: bool, ends_with_vowel: bool) -> u32 {
        // Harakat per base letter of the corpus form.
        let mut letters: Vec<(char, Option<char>)> = Vec::new();
        for ch in vocalized.chars() {
            let vowel = match ch {
                '\u{064E}' | '\u{064B}' => Some('a'),
                '\u{0650}' | '\u{064D}' => Some('i'),
                '\u{064F}' | '\u{064C}' => Some('u'),
                _ => None,
            };
            let is_mark = matches!(ch as u32, 0x064B..=0x0652 | 0x0670 | 0x0610..=0x061A);
            if is_mark {
                if let (Some(v), Some(last)) = (vowel, letters.last_mut()) {
                    last.1 = Some(v);
                }
            } else {
                letters.push((ch, None));
            }
        }
        if letters.is_empty() {
            return 0;
        }
        // The final letter's haraka is usually a case ending nobody types —
        // unless the Latin ends in a vowel that sits on a final consonant
        // (`enta` → أنتَ, `howa` → هوَ).
        let last_idx = letters.len() - 1;
        let keep_last = ends_with_vowel && !matches!(letters[last_idx].0, 'ا' | 'ى' | 'ة');
        if !keep_last {
            letters[last_idx].1 = None;
        }
        // A typed initial vowel corresponds to the initial alef's haraka.
        if starts_with_vowel && matches!(letters[0].0, 'ا' | 'أ' | 'إ' | 'آ') {
            letters[0].1 = None;
        }
        // Fatha followed by a vowelless ي / و is a diphthong (بَيْت, يَوْم),
        // which dialects pronounce — and Arabizi writes — as ee / oo.
        let mut arabic: Vec<char> = Vec::new();
        for (idx, (_, v)) in letters.iter().enumerate() {
            let Some(v) = *v else { continue };
            let next = letters.get(idx + 1);
            let class = match (v, next) {
                ('a', Some(('ي', None))) => 'Y',
                ('a', Some(('و', None))) => 'W',
                _ => v,
            };
            arabic.push(class);
        }

        // Contradiction score: 1 per clear clash, ½ per soft one.
        let clash = |t: char, a: char| -> u32 {
            match (t, a) {
                ('a', 'a' | 'Y' | 'W') => 0,
                ('i', 'i' | 'Y') => 0,
                ('e', 'i' | 'Y') => 0,
                ('e', 'a' | 'u' | 'W') => 1,
                ('u', 'u' | 'W') => 0,
                ('y', 'Y' | 'i') => 0,
                ('y', 'a') => 1,
                ('w', 'W' | 'u') => 0,
                ('w', 'a') => 1,
                // Egyptian o/u for a classical a (noss → نَصّ, 2oddam → قُدَّام).
                ('u', 'a') => 1,
                _ => 2,
            }
        };
        let halves: u32 = if typed.len() > arabic.len() {
            2
        } else if typed.len() == arabic.len() {
            typed.iter().zip(&arabic).map(|(t, a)| clash(*t, *a)).sum()
        } else {
            0
        };
        halves.min(4)
    }

    /// Bonus for single words attested in colloquial text, so spoken forms
    /// (فاهم, قمر, شقة) can beat formal words that dominate the main lists.
    fn dialect_score(&self, candidate: &str) -> i64 {
        if candidate.contains(' ') {
            return 0;
        }
        self.dialect
            .count(&strip_tashkeel(candidate))
            .map(|c| (DIALECT_WEIGHT * 1000.0 * (c as f64 + 1.0).log10()) as i64)
            .unwrap_or(0)
    }

    /// Language-model score of a candidate in milli-units:
    /// `1000 × LM_WEIGHT × log10(count)`, averaged over its words.
    ///
    /// Both lookup tables key on bare (unvocalized) forms — OpenSubtitles is
    /// already stripped, Tashkeela is indexed by stripped base. Candidates
    /// carry tanween (e.g. طبعاً), so strip before looking up, or every
    /// tanween candidate misses the frequency signal.
    ///
    /// OpenSubtitles is the primary signal. Tashkeela (MSA/classical) is a
    /// fallback for words OpenSubtitles lacks, discounted because it is
    /// MSA-heavy and would otherwise outrank dialectal forms. Unknown words
    /// score below anything attested.
    fn lm_score(&self, candidate: &str) -> i64 {
        let words: Vec<String> = candidate.split_whitespace().map(strip_tashkeel).collect();
        if words.is_empty() {
            return LM_UNKNOWN as i64;
        }
        let total: f64 = words
            .iter()
            .map(|w| {
                if let Some(c) = self.frequency.count(w) {
                    LM_WEIGHT * 1000.0 * (c.max(1) as f64).log10()
                } else if let Some(c) = self.tashkeela.count(w) {
                    LM_WEIGHT * 1000.0 * (c.max(1) as f64).log10() - LM_TASHKEELA_PENALTY
                } else {
                    LM_UNKNOWN
                }
            })
            .sum();
        (total / words.len() as f64) as i64
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

    fn match_picker_max(
        input: &str,
        max_len: usize,
    ) -> Option<(&'static str, &'static str, &'static [&'static str])> {
        PICKER_ALTERNATIVES
            .iter()
            .filter(|(pattern, _, _)| pattern.len() <= max_len && input.starts_with(pattern))
            .max_by_key(|(pattern, _, _)| pattern.len())
            .map(|(pattern, primary, alternatives)| (*pattern, *primary, *alternatives))
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

/// Strip Arabic combining-mark code points (tashkeel) from a string. Used when
/// validating picker overrides so a base-letter override is recognized even
/// when the user has attached fatha/kasra/damma/shadda/etc.
fn strip_tashkeel(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(*c as u32, 0x064B..=0x0652 | 0x0670 | 0x0610..=0x061A))
        .collect()
}

/// A position in the Latin input where the user can pick from multiple Arabic
/// letters. `pos` is the starting char index of the matched pattern in the
/// lowercased input; `pattern` is the Latin sequence (e.g. "z", "th").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LetterSlot {
    pub pos: usize,
    pub pattern: String,
    pub primary: String,
    pub alternatives: Vec<String>,
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
    fn no_latin_letters_leak_into_output() {
        let e = engine();
        for word in ["spaced", "abc", "cairo", "allcaps"] {
            let results = e.transliterate_word_ranked(word, None);
            let top = results.first().expect("at least one candidate");
            assert!(!top.chars().any(|c| c.is_ascii_alphabetic()), "Latin letter left in {top:?} for {word:?}");
        }
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
    fn number_mappings_work_inside_word() {
        let e = engine();
        // Inside a word, '7' must still be the consonant ح.
        let results = e.transliterate_word("saba7");
        assert!(results.iter().any(|r| r.contains("ح")),
            "Expected ح inside 'saba7', got {:?}", results);
    }

    #[test]
    fn single_bare_digit_is_a_number() {
        let e = engine();
        // Standalone "7" is the digit, not the consonant — per the rule that
        // whole-token digits are numbers, in-word digits are consonants.
        let results = e.transliterate_word("7");
        assert_eq!(results, vec!["٧".to_string()]);
    }

    #[test]
    fn digraph_sh() {
        let e = engine();
        let results = e.transliterate_word("sh");
        assert!(results.iter().any(|r| r.contains("ش")),
            "Expected ش for 'sh', got {:?}", results);
    }

    #[test]
    fn logical_spelling_resolves_ambiguous_digraph_and_final_alef() {
        let e = engine();
        let results = e.transliterate_word("hatha");
        assert_eq!(results.first().map(String::as_str), Some("هذا"),
            "Expected هذا on top for 'hatha', got {:?}", results);

        let slots = e.letter_slots("hatha");
        assert!(slots.iter().any(|slot| slot.pos == 2 && slot.pattern == "th"),
            "Expected the picker to tokenize the longest `th` pattern, got {:?}", slots);
    }

    #[test]
    fn common_logical_spellings_rank_expected_word_first() {
        let e = engine();
        let cases = [
            ("hatha", "هذا"),
            ("hadha", "هذا"),
            ("haza", "هذا"),
            ("thahab", "ذهب"),
            ("lazeez", "لذيذ"),
            ("gameela", "جميلة"),
        ];

        for (latin, expected) in cases {
            let results = e.transliterate_word(latin);
            assert_eq!(results.first().map(String::as_str), Some(expected),
                "Expected `{}` to rank first for `{}`; got {:?}",
                expected, latin, results);
        }
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
    fn cldr_lookup_laugh() {
        let e = engine();
        // ضحك (laugh) should now surface laugh emojis via CLDR
        let emojis = e.lookup_emojis(&["ضحك".to_string()]);
        assert!(!emojis.is_empty(), "Expected emojis for ضحك, got none");
        assert!(emojis.len() <= 3, "Should return at most 3 emojis");
    }

    #[test]
    fn cldr_lookup_egg() {
        let e = engine();
        // بيض (eggs) should surface an egg emoji
        let emojis = e.lookup_emojis(&["بيض".to_string()]);
        assert!(!emojis.is_empty(), "Expected emojis for بيض, got none");
    }

    #[test]
    fn cldr_lookup_plant() {
        let e = engine();
        // نبات (plant) should surface a plant emoji
        let emojis = e.lookup_emojis(&["نبات".to_string()]);
        assert!(!emojis.is_empty(), "Expected emojis for نبات, got none");
    }

    #[test]
    fn emoticon_smiley() {
        let e = engine();
        let r = e.lookup_emoticon(":)").unwrap();
        assert!(r.iter().any(|s| s.contains("😊") || s.contains("🙂")),
            "Expected smiley emoji for ':)', got {:?}", r);
    }

    #[test]
    fn emoticon_heart() {
        let e = engine();
        let r = e.lookup_emoticon("<3").unwrap();
        assert!(r.iter().any(|s| s.contains("❤") || s.contains("🥰")),
            "Expected heart emoji for '<3', got {:?}", r);
    }

    #[test]
    fn emoticon_xd() {
        let e = engine();
        let r = e.lookup_emoticon("xd").unwrap();
        assert!(r.iter().any(|s| s.contains("😂") || s.contains("🤣")),
            "Expected laugh emoji for 'xd', got {:?}", r);
    }

    #[test]
    fn emoticon_no_match_returns_none() {
        let e = engine();
        assert!(e.lookup_emoticon("salam").is_none());
    }

    #[test]
    fn alef_layyina_variant_generated() {
        let e = engine();
        // "t3aalaa" → primary should be تعالى (not تعالا)
        let results = e.transliterate("t3aalaa");
        assert!(results.iter().any(|r| r == "تعالى"),
            "Expected تعالى in candidates for 't3aalaa', got {:?}", results);
        assert_eq!(results[0], "تعالى",
            "Expected تعالى as top candidate for 't3aalaa', got {:?}", results);
    }

    #[test]
    fn alef_layyina_variant_for_rule_based() {
        let e = engine();
        // Any word ending in ا should also offer ى variant
        let results = e.transliterate_word("3alaa");
        assert!(results.iter().any(|r| r.ends_with('ى')),
            "Expected a ى-ending candidate for '3alaa', got {:?}", results);
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
    fn letter_slots_finds_ambiguous_consonants() {
        let e = engine();
        let slots = e.letter_slots("baalzbt");
        // Expect slots for: b (no, primary only — wait, b is not ambiguous)
        // Ambiguous patterns in "baalzbt": z (alts ظ ذ), t (alts ط ث), and also b/h/etc are not ambiguous
        let patterns: Vec<&str> = slots.iter().map(|s| s.pattern.as_str()).collect();
        assert!(patterns.contains(&"z"), "expected 'z' slot in {:?}", patterns);
        assert!(patterns.contains(&"t"), "expected 't' slot in {:?}", patterns);
    }

    #[test]
    fn override_pins_balzbt_to_top() {
        let e = engine();
        let slots = e.letter_slots("baalzbt");
        let z_slot = slots.iter().find(|s| s.pattern == "z").unwrap();
        let t_slot = slots.iter().find(|s| s.pattern == "t").unwrap();
        let mut overrides = HashMap::new();
        overrides.insert(z_slot.pos, "ظ".to_string());
        overrides.insert(t_slot.pos, "ط".to_string());
        let results = e.transliterate_word_with_overrides("baalzbt", &overrides, None);
        assert_eq!(results[0], "بالظبط",
            "expected بالظبط pinned at top, got {:?}", results);
    }

    #[test]
    fn letter_slots_includes_every_letter_token() {
        let e = engine();
        let slots = e.letter_slots("baalzbt");
        // b, aa, l, z, b, t  → 6 tokens
        let patterns: Vec<&str> = slots.iter().map(|s| s.pattern.as_str()).collect();
        assert_eq!(patterns, vec!["b", "aa", "l", "z", "b", "t"]);
        // Every consonant gets a slot, even if alternatives is empty (b, l).
        let b = slots.iter().find(|s| s.pattern == "b").unwrap();
        assert!(b.alternatives.is_empty(), "b should have no alternatives");
        let aa = slots.iter().find(|s| s.pattern == "aa").unwrap();
        assert!(!aa.alternatives.is_empty(), "aa should offer ى/آ alternatives");
    }

    #[test]
    fn override_for_vowel_works() {
        let e = engine();
        let slots = e.letter_slots("3ali");
        // The 'a' slot should be overridable to ع
        let a_slot = slots.iter().find(|s| s.pattern == "a").unwrap();
        let mut overrides = HashMap::new();
        overrides.insert(a_slot.pos, "ع".to_string());
        let results = e.transliterate_word_with_overrides("3ali", &overrides, None);
        // First candidate should contain two ع (the 3 + the override)
        assert!(results[0].matches("ع").count() >= 2,
            "expected two ع in pinned candidate, got {}", results[0]);
    }

    #[test]
    fn every_declared_picker_choice_can_be_applied() {
        let e = engine();
        for (pattern, primary, alternatives) in PICKER_ALTERNATIVES {
            let slots = e.letter_slots(pattern);
            let slot = slots
                .iter()
                .find(|slot| slot.pos == 0 && slot.pattern == *pattern)
                .unwrap_or_else(|| panic!("picker did not preserve longest slot for `{}`", pattern));

            for choice in std::iter::once(primary).chain(alternatives.iter()) {
                let mut overrides = HashMap::new();
                overrides.insert(slot.pos, (*choice).to_string());
                let results = e.transliterate_word_with_overrides(pattern, &overrides, None);
                assert_eq!(results.first().map(String::as_str), Some(*choice),
                    "picker choice `{}` could not be applied to `{}`; got {:?}",
                    choice, pattern, results);
            }
        }
    }

    #[test]
    fn egyptian_participles_get_long_a() {
        // CaCeC is the spoken active participle; the verb is said with e.
        let e = engine();
        for (input, want) in [("fahem", "فاهم"), ("nazel", "نازل"), ("faker", "فاكر"), ("3aref", "عارف"), ("ghali", "غالي")] {
            let top = e.transliterate_word_ranked(input, None);
            assert_eq!(top.first().map(String::as_str), Some(want), "{input}: {top:?}");
        }
        // Same shape, but ordinary words that must not gain an alef.
        for (input, want) in [("hadef", "هدف"), ("kalem", "كلام"), ("sadiq", "صديق"), ("3ali", "علي"), ("kabed", "كبد"), ("nafes", "نفس")] {
            let top = e.transliterate_word_ranked(input, None);
            assert_eq!(top.first().map(String::as_str), Some(want), "{input}: {top:?}");
        }
    }

    #[test]
    fn user_pick_beats_default_ranking() {
        let e = engine();
        let default = e.transliterate_word_ranked("7abibi", None);
        let other = default.get(1).expect("needs a second candidate").clone();
        let mut prefs = UserPreferences::new();
        prefs.record("7abibi", &other);
        let ranked = e.transliterate_word_ranked("7abibi", Some(&prefs));
        assert_eq!(ranked.first(), Some(&other), "user's pick should come first: {ranked:?}");
    }

    #[test]
    fn user_pref_resurfaces_picker_derived_candidate() {
        let e = engine();
        let mut prefs = UserPreferences::new();
        prefs.record("baalzbt", "بالظبط");
        let results = e.transliterate_word_ranked("baalzbt", Some(&prefs));
        assert_eq!(results[0], "بالظبط",
            "expected previously-confirmed بالظبط on top, got {:?}", results);
    }

    #[test]
    fn override_with_no_changes_returns_normal_ranking() {
        let e = engine();
        let baseline = e.transliterate_word_ranked("salam", None);
        let with_empty = e.transliterate_word_with_overrides("salam", &HashMap::new(), None);
        assert_eq!(baseline, with_empty);
    }

    #[test]
    fn digit_token_converts_to_arabic_indic() {
        let e = engine();
        let results = e.transliterate_word("2026");
        assert_eq!(results, vec!["٢٠٢٦".to_string()]);
    }

    #[test]
    fn digit_token_with_decimal_point() {
        let e = engine();
        let results = e.transliterate_word("100.5");
        assert_eq!(results, vec!["١٠٠.٥".to_string()]);
    }

    #[test]
    fn mixed_digit_letter_still_uses_consonant_mapping() {
        let e = engine();
        let results = e.transliterate_word("7abibi");
        // Should still produce the dictionary form, not Arabic-Indic digits.
        assert!(results.iter().any(|r| r == "حبيبي"),
            "expected حبيبي for '7abibi', got {:?}", results);
    }

    #[test]
    fn tashkeel_override_pins_with_fatha() {
        let e = engine();
        // "bb" → primary بب; override pos 0 with بَ (ba+fatha) should pin بَب.
        let slots = e.letter_slots("bb");
        let b0 = slots.iter().find(|s| s.pos == 0).unwrap();
        assert_eq!(b0.primary, "ب");
        let mut overrides = HashMap::new();
        overrides.insert(b0.pos, "بَ".to_string());
        let results = e.transliterate_word_with_overrides("bb", &overrides, None);
        assert_eq!(results[0], "بَب",
            "expected fatha to pin on first ب, got {:?}", results);
    }

    #[test]
    fn digraph_span_override_collapses_to_single_letter() {
        let e = engine();
        // "kheir" splits into slots k, h, ei, r (engine treats k/h as separate
        // picker slots even though kh→خ is a digraph). The combined picker
        // emits {k.pos: "خ", h.pos: ""} so the two Latin slots collapse to خ.
        let slots = e.letter_slots("kheir");
        let k = slots.iter().find(|s| s.pattern == "k").unwrap();
        let h = slots.iter().find(|s| s.pattern == "h").unwrap();
        let mut overrides = HashMap::new();
        overrides.insert(k.pos, "خ".to_string());
        overrides.insert(h.pos, String::new());
        let results = e.transliterate_word_with_overrides("kheir", &overrides, None);
        assert_eq!(results[0], "خير",
            "expected خير from digraph span override, got {:?}", results);
    }

    #[test]
    fn collapsed_span_choice_is_validated_as_a_group() {
        let e = engine();
        let slots = e.letter_slots("sh");
        let s = slots.iter().find(|slot| slot.pattern == "s").unwrap();
        let h = slots.iter().find(|slot| slot.pattern == "h").unwrap();
        let mut overrides = HashMap::new();
        // The fatha makes this observably different from falling back to the
        // normal sh -> ش rule after silently rejecting the span override.
        overrides.insert(s.pos, "شَ".to_string());
        overrides.insert(h.pos, String::new());
        let results = e.transliterate_word_with_overrides("sh", &overrides, None);
        assert_eq!(results.first().map(String::as_str), Some("شَ"),
            "collapsed span override was ignored; got {:?}", results);
    }

    #[test]
    fn tashkeel_override_with_shadda_kasra() {
        let e = engine();
        let slots = e.letter_slots("salam");
        let s_slot = slots.iter().find(|s| s.pattern == "s").unwrap();
        let mut overrides = HashMap::new();
        // shadda + kasra on س
        overrides.insert(s_slot.pos, "سِّ".to_string());
        let results = e.transliterate_word_with_overrides("salam", &overrides, None);
        assert!(results[0].contains("سِّ") || results[0].contains("سّ"),
            "expected shadda+kasra to survive on س, got {:?}", results);
    }

    #[test]
    fn tashkeela_lookup_returns_none_for_unknown() {
        let e = engine();
        // Bogus Arabic string should never appear in Tashkeela.
        assert!(e.lookup_vocalized("زززززز").is_none());
    }

    #[test]
    fn tashkeela_lookup_present_when_corpus_loaded() {
        let e = engine();
        // If the embedded corpus is non-empty, at least one of these very
        // common MSA words should resolve to a vocalized form. The check is
        // gated so the test still passes on a fresh checkout where the data
        // file is the empty placeholder.
        let probes = ["في", "من", "الله", "على", "إلى"];
        let any_known = probes.iter().any(|w| e.frequency.rank(w).is_some());
        if !any_known {
            return; // engine not configured — skip
        }
        let corpus_populated = probes.iter().any(|w| e.lookup_vocalized(w).is_some());
        if corpus_populated {
            // At least one probe must round-trip: lookup returns a string whose
            // tashkeel-stripped form equals the probe.
            let hit = probes.iter()
                .find_map(|w| e.lookup_vocalized(w).map(|v| (w, v)));
            assert!(hit.is_some(), "expected at least one corpus hit");
            let (probe, vocalized) = hit.unwrap();
            let stripped: String = vocalized.chars()
                .filter(|c| !matches!(*c as u32, 0x064B..=0x0652 | 0x0670 | 0x0610..=0x061A))
                .collect();
            assert_eq!(stripped, *probe);
        }
    }

    #[test]
    fn frequency_lookup_strips_tashkeel() {
        let e = engine();
        // "tb3n" should resolve to طبعاً (very common) not تبعاً (rare).
        // Both candidates carry tanween, so without tashkeel-stripping in the
        // frequency lookup neither hits the freq list and ranking falls back
        // to insertion order — which puts the wrong one (تبعاً) on top.
        let results = e.transliterate_word("tb3n");
        assert_eq!(results[0], "طبعاً",
            "expected طبعاً on top for 'tb3n', got {:?}", results);
    }

    #[test]
    fn tanween_applies_to_consonant_swaps() {
        let e = engine();
        // "tb3an" is the other natural spelling; tanween generation has to
        // see the ط swap candidate (طبعن) so it can emit طبعاً.
        let results = e.transliterate_word("tb3an");
        assert_eq!(results[0], "طبعاً",
            "expected طبعاً on top for 'tb3an', got {:?}", results);
    }

    #[test]
    fn tashkeela_ranking_keeps_existing_winners() {
        let e = engine();
        // The corpus must not unseat established dictionary picks. "shukran"
        // → شكرًا is a hand-curated entry; ranking changes shouldn't displace it.
        let results = e.transliterate("shukran");
        assert_eq!(results[0], "شكرًا",
            "Tashkeela ranking displaced the dictionary entry: {:?}", results);
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
