//! Turns raw input into per-word candidate lists (the old `transliterate` and
//! `transliterate_with_overrides` Tauri commands).

use crate::text::{extract_punctuation, strip_bidi_controls};
use arabizi_engine::{TransliterationEngine, UserPreferences};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct WordResult {
    pub input: String,
    pub candidates: Vec<String>,
    pub emojis: Vec<String>,
}

/// One result per whitespace-separated word. Letter `overrides` (keyed by
/// Latin char position) apply only when the input is a single word.
pub fn transliterate(
    engine: &TransliterationEngine,
    prefs: &UserPreferences,
    input: &str,
    overrides: Option<&HashMap<usize, String>>,
) -> Vec<WordResult> {
    let lower = strip_bidi_controls(input.trim()).to_lowercase();
    if lower.is_empty() {
        return Vec::new();
    }
    let parts: Vec<&str> = lower.split_whitespace().collect();
    let single = parts.len() == 1;
    parts
        .iter()
        .map(|w| {
            // Emoticons are made of punctuation — check before stripping it.
            if let Some(emojis) = engine.lookup_emoticon(w) {
                return WordResult { input: w.to_string(), candidates: emojis, emojis: vec![] };
            }
            let (prefix, core, suffix) = extract_punctuation(w);
            if core.is_empty() {
                return WordResult {
                    input: w.to_string(),
                    candidates: vec![format!("{prefix}{suffix}")],
                    emojis: vec![],
                };
            }
            let raw = match overrides {
                Some(ov) if single && !ov.is_empty() => {
                    engine.transliterate_word_with_overrides(&core, ov, Some(prefs))
                }
                _ => engine.transliterate_word_ranked(&core, Some(prefs)),
            };
            let emojis = engine.lookup_emojis(&raw);
            let candidates = raw.into_iter().map(|c| format!("{prefix}{c}{suffix}")).collect();
            WordResult { input: w.to_string(), candidates, emojis }
        })
        .collect()
}
