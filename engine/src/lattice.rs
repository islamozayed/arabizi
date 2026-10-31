//! Weighted candidate lattice ("noisy channel" generation).
//!
//! The Latin word is split into segments (consonants, vowels, hamza, …). Each
//! segment offers one or more Arabic spellings, each with a *cost* expressing
//! how unlikely it is that an Arabizi writer would type this Latin for that
//! Arabic (0 = the canonical reading). The engine enumerates the cheapest
//! combinations and ranks them by `language-model score − cost`, so a real,
//! frequent word can beat the literal reading only when the evidence is
//! strong enough.
//!
//! Costs are in milli-units where 1000 ≈ one order of magnitude of word
//! frequency (see `TransliterationEngine::lm_score`).

use crate::mappings::{is_vowel_char, AMBIGUOUS_CONSONANTS, CONSONANT_MAPPINGS, LONG_VOWEL_MAPPINGS};

/// One Arabic option for a segment.
type Opt = (String, i32);
/// All options for one Latin segment.
type Segment = Vec<Opt>;

/// Cost of reading a doubled Latin consonant ("ll", "dd") as two Arabic
/// letters instead of one letter (shadda is never written in chat).
const DOUBLE_LETTER: i32 = 500;
/// Cost of writing a medial short Latin vowel as a long-vowel letter. Kept
/// low: Arabizi writers usually type long vowels as a single letter
/// (salamat, 3amel), so the language model and vowel agreement decide.
const MEDIAL_LONG_A: i32 = 600;
const MEDIAL_LONG_I: i32 = 500;
const MEDIAL_LONG_E: i32 = 700;
const MEDIAL_LONG_O: i32 = 500;
/// A vowel in the last syllable (just before the final consonant) is very
/// often long in Arabic (salaam, kitaab, gameel), so lengthening it is cheaper.
const LAST_SYLLABLE_DISCOUNT: i32 = 300;
/// Long ā in the active-participle shape CaCeC (fahem → فاهم, nazel → نازل,
/// 3aref → عارف). Spoken Arabic uses these constantly and Arabizi writes
/// their long ā as a single `a`. It is a bonus, not just a cheaper reading:
/// Egyptian says the verb with e (fehem → فهم), so `a` here is strong
/// evidence for the participle even when the verb is far more frequent.
/// Tuned so fahem/nazel/faker win while hadef/kalem/kabed/nafes keep their
/// short reading (see `egyptian_participles_get_long_a`).
const PARTICIPLE_LONG_A: i32 = -1100;
/// Weaker version for CaCi (ghali → غالي, mashi → ماشي), which also matches
/// words like yabi (يبي) and names like 3ali (علي).
const PARTICIPLE_LONG_A_FINAL_I: i32 = 100;

/// Where a participle-shaped word has its `a`, and which shape it is.
#[derive(Clone, Copy, PartialEq)]
enum Participle {
    /// C a C e C
    Strong(usize),
    /// C a C i
    FinalI(usize),
}

/// Recognise the participle shapes CaCeC and CaCi.
fn participle_a(chars: &[char]) -> Option<Participle> {
    let consonant = |i: usize| -> Option<usize> {
        let c = *chars.get(i)?;
        let next = chars.get(i + 1).copied();
        if matches!((c, next), ('s' | 'k' | 'g' | 't' | 'd', Some('h')) | ('3' | '6' | '7' | '9', Some('\''))) {
            return Some(i + 2);
        }
        (c.is_ascii_alphanumeric() && !is_vowel_char(c)).then_some(i + 1)
    };
    let i = consonant(0)?;
    if chars.get(i) != Some(&'a') {
        return None;
    }
    let j = consonant(i + 1)?;
    match chars.get(j) {
        // CaCeC: exactly one consonant after the e. (CaCiC is usually the
        // long-ī adjective shape instead: sadiq → صديق, kabir → كبير.)
        Some('e') if j + 1 < chars.len() => {
            let k = consonant(j + 1)?;
            (k == chars.len()).then_some(Participle::Strong(i))
        }
        // CaCi: defective participle (ghali, mashi, nasi).
        Some('i') if j + 1 == chars.len() => Some(Participle::FinalI(i)),
        _ => None,
    }
}

/// Egyptian/Levantine `2` for ق (2albi → قلبي, de2i2a → دقيقة).
const HAMZA_AS_QAF: i32 = 500;
/// Tanween fatha for a final `n` (shukran → شكراً).
const TANWEEN: i32 = 300;
/// Dropping a typed final vowel entirely (ayna → أين).
const FINAL_VOWEL_DROP: i32 = 1500;

fn swap_cost(pattern: &str, alt: &str) -> i32 {
    match (pattern, alt) {
        ("s", "ص") => 500,
        ("s", "ث") => 1300,
        ("t", "ط") => 800,
        ("t", "ث") => 1100,
        ("d", "ض") => 800,
        ("d", "ذ") => 1200,
        ("z", "ظ") => 800,
        ("z", "ذ") => 900,
        ("h", "ح") => 800,
        ("g", "غ") => 1500,
        ("g", "ق") => 1200,
        ("k", "ق") => 1600,
        ("c", _) => 1300,
        ("th", "ذ") => 800,
        ("dh", _) => 900,
        ("ch", _) => 1000,
        _ => 1200,
    }
}

fn match_consonant(input: &str, max_len: usize) -> Option<(&'static str, &'static str)> {
    CONSONANT_MAPPINGS
        .iter()
        .find(|(p, _)| p.len() <= max_len && input.starts_with(p))
        .map(|(p, a)| (*p, *a))
}

fn match_long_vowel(input: &str) -> Option<&'static str> {
    LONG_VOWEL_MAPPINGS
        .iter()
        .find(|(p, _)| input.starts_with(p))
        .map(|(p, _)| *p)
}

fn opts(list: &[(&str, i32)]) -> Segment {
    list.iter().map(|(a, c)| (a.to_string(), *c)).collect()
}

/// Seat of a mid-word hamza from the strongest adjacent vowel
/// (kasra > damma > fatha), mirroring `TransliterationEngine::resolve_hamza`.
fn hamza_seat(chars: &[char], pos: usize) -> &'static str {
    let strength = |c: Option<&char>| match c {
        Some('i') | Some('e') => 3,
        Some('u') | Some('o') => 2,
        _ => 1,
    };
    let s = strength(pos.checked_sub(1).and_then(|p| chars.get(p))).max(strength(chars.get(pos + 1)));
    match s {
        3 => "ئ",
        2 => "ؤ",
        _ => "أ",
    }
}

const SUN_LETTERS: &[char] = &['t', 'd', 'r', 'z', 's', 'n'];

/// Build the lattice for a lowercased word.
pub fn build(word: &str) -> Vec<Segment> {
    let chars: Vec<char> = word.chars().collect();
    let len = chars.len();
    let mut segs: Vec<Segment> = Vec::new();
    let mut pos = 0;

    // Index of the last letter-ish char (ignores trailing separators).
    let last = chars.iter().rposition(|c| *c != '-').unwrap_or(0);
    let participle = participle_a(&chars);

    while pos < len {
        let c = chars[pos];
        if c == '-' {
            pos += 1;
            continue;
        }
        let at_start = pos == 0;
        let has_separator = pos + 1 < len && chars[pos + 1] == '-';
        let remaining: String = chars[pos..].iter().collect();
        let max_len = if has_separator { 1 } else { usize::MAX };

        // --- Hamza / Egyptian qaf ---
        if c == '2' {
            let seg = if at_start {
                let seat = match chars.get(1) {
                    Some('i') | Some('e') => "إ",
                    _ => "أ",
                };
                opts(&[(seat, 0), ("ا", 300), ("ق", HAMZA_AS_QAF)])
            } else if pos == last {
                opts(&[("ء", 0), ("أ", 300), ("ئ", 500), ("ق", HAMZA_AS_QAF)])
            } else {
                let seat = hamza_seat(&chars, pos);
                let mut s = vec![(seat.to_string(), 0)];
                for alt in ["أ", "ئ", "ؤ", "ء"] {
                    if alt != seat {
                        s.push((alt.to_string(), 400));
                    }
                }
                s.push(("ق".to_string(), HAMZA_AS_QAF));
                s
            };
            segs.push(seg);
            pos += 1;
            continue;
        }

        // --- Consonants (incl. digraphs and number-letters) ---
        if let Some((pattern, primary)) = match_consonant(&remaining, max_len) {
            let plen = pattern.len();
            let mut seg: Segment = vec![(primary.to_string(), 0)];
            if let Some((_, _, alts)) = AMBIGUOUS_CONSONANTS
                .iter()
                .filter(|(p, _, _)| p.len() <= max_len && remaining.starts_with(p))
                .max_by_key(|(p, _, _)| p.len())
                .filter(|(p, _, _)| p.len() == plen)
            {
                for alt in *alts {
                    seg.push((alt.to_string(), swap_cost(pattern, alt)));
                }
            }

            // Doubled Latin consonant → one Arabic letter (shadda) by default.
            let doubled = plen == 1
                && !has_separator
                && chars.get(pos + 1) == Some(&c)
                && c.is_ascii_alphabetic();
            if doubled {
                let doubles: Segment = seg
                    .iter()
                    .map(|(a, cost)| (format!("{a}{a}"), cost + DOUBLE_LETTER))
                    .collect();
                seg.extend(doubles);
            }
            let end = pos + if doubled { 2 } else { plen };
            let is_final = end > last;

            // Word-final `h` after a vowel may be taa marbuta (7ayah → حياة).
            if is_final && pattern == "h" && pos > 0 && matches!(chars[pos - 1], 'a' | 'e') {
                seg.push(("ة".to_string(), 300));
            }
            // Word-final `n` may be tanween fatha (shukran → شكراً), but only
            // after a short `a` or a consonant — never after a long vowel
            // (`meen` is مين, not مياً).
            let prev = chars[..pos].iter().rev().find(|c| **c != '-').copied();
            let prev2 = pos.checked_sub(2).map(|p| chars[p]);
            let tanween_ok = match prev {
                Some('a') => !prev2.is_some_and(is_vowel_char),
                Some(p) => !is_vowel_char(p),
                None => false,
            };
            if is_final && pattern == "n" && pos >= 2 && tanween_ok {
                seg.push(("اً".to_string(), TANWEEN));
            }

            segs.push(seg);
            pos = end;
            if has_separator && pos < len && chars[pos] == '-' {
                pos += 1;
            }
            continue;
        }

        // --- Long vowels (aa, ee, oo, ou, ei, ai, …) ---
        if let Some(pattern) = match_long_vowel(&remaining) {
            let end = pos + pattern.len();
            let is_final = end > last;
            let seg = if at_start {
                match pattern {
                    "aa" => opts(&[("ا", 0), ("آ", 300), ("أ", 200)]),
                    "ee" | "ii" | "ei" => opts(&[("إي", 0), ("اي", 100), ("أي", 300)]),
                    "ai" => opts(&[("أي", 0), ("اي", 100), ("إي", 400)]),
                    _ => opts(&[("أو", 0), ("او", 200)]),
                }
            } else {
                match pattern {
                    "aa" if is_final => opts(&[("ا", 0), ("ى", 200), ("اء", 700)]),
                    "aa" => opts(&[("ا", 0)]),
                    "ee" | "ii" if is_final => opts(&[("ي", 0), ("ى", 600)]),
                    "ee" | "ii" => opts(&[("ي", 0)]),
                    "ei" => opts(&[("ي", 0), ("اي", 600)]),
                    "ai" => opts(&[("ي", 0), ("اي", 300)]),
                    _ => opts(&[("و", 0), ("او", 400)]),
                }
            };
            segs.push(seg);
            pos = end;
            continue;
        }

        // --- Short vowels ---
        if is_vowel_char(c) {
            let is_final = pos == last;
            let after_consonant = pos > 0 && !is_vowel_char(chars[pos - 1]);
            let seg = if at_start {
                // Sun-letter assimilation: "essalam" → السلام, "ennas" → الناس.
                let assimilated = len > 3
                    && SUN_LETTERS.contains(&chars[1])
                    && chars[2] == chars[1];
                let mut s = match c {
                    'a' => opts(&[("ا", 0), ("أ", 100), ("إ", 600)]),
                    // `el` / `il` + … is overwhelmingly the article ال.
                    'e' | 'i' if chars.get(1) == Some(&'l') => opts(&[("ا", 0), ("إ", 100), ("أ", 500)]),
                    'e' => opts(&[("إ", 0), ("ا", 100), ("أ", 500)]),
                    'i' => opts(&[("إ", 0), ("ا", 100)]),
                    _ => opts(&[("أ", 0), ("ا", 300), ("أو", 400)]),
                };
                if assimilated {
                    s.push(("ال".to_string(), 200));
                }
                s
            } else if is_final {
                if after_consonant {
                    match c {
                        'a' => opts(&[("ة", 0), ("ا", 300), ("ى", 500), ("ه", 900), ("", FINAL_VOWEL_DROP)]),
                        'e' => opts(&[("ة", 100), ("ي", 300), ("ا", 600), ("ى", 700), ("ه", 800), ("", FINAL_VOWEL_DROP)]),
                        'i' => opts(&[("ي", 0), ("ى", 700), ("", FINAL_VOWEL_DROP)]),
                        _ => opts(&[("و", 0), ("ه", 700), ("", FINAL_VOWEL_DROP)]),
                    }
                } else {
                    match c {
                        'a' | 'e' => opts(&[("ا", 0), ("ى", 300), ("ة", 500), ("", 800)]),
                        'i' => opts(&[("ي", 0), ("", 800)]),
                        _ => opts(&[("و", 0), ("", 800)]),
                    }
                }
            } else {
                // Medial vowel: usually a short vowel (unwritten), sometimes long.
                let last_syllable = pos + 1 == last;
                let discount = if last_syllable { LAST_SYLLABLE_DISCOUNT } else { 0 };
                match c {
                    'a' if participle == Some(Participle::Strong(pos)) => opts(&[("", 0), ("ا", PARTICIPLE_LONG_A)]),
                    'a' if participle == Some(Participle::FinalI(pos)) => opts(&[("", 0), ("ا", PARTICIPLE_LONG_A_FINAL_I)]),
                    'a' => opts(&[("", 0), ("ا", MEDIAL_LONG_A - discount)]),
                    'e' => opts(&[("", 0), ("ي", MEDIAL_LONG_E - discount), ("ا", 1000 - discount)]),
                    'i' => opts(&[("", 0), ("ي", MEDIAL_LONG_I - discount)]),
                    _ => opts(&[("", 0), ("و", MEDIAL_LONG_O - discount)]),
                }
            };
            segs.push(seg);
            pos += 1;
            continue;
        }

        // --- Anything else is kept verbatim ---
        segs.push(vec![(c.to_string(), 0)]);
        pos += 1;
    }

    segs
}

/// Enumerate the `beam` cheapest full paths through the lattice.
pub fn enumerate(segs: &[Segment], beam: usize) -> Vec<Opt> {
    let mut paths: Vec<Opt> = vec![(String::new(), 0)];
    for seg in segs {
        let mut next: Vec<Opt> = Vec::with_capacity(paths.len() * seg.len());
        for (prefix, cost) in &paths {
            for (ar, c) in seg {
                next.push((format!("{prefix}{ar}"), cost + c));
            }
        }
        next.sort_by_key(|(_, c)| *c);
        // Dedupe identical strings, keeping the cheapest.
        let mut seen = std::collections::HashSet::new();
        next.retain(|(s, _)| seen.insert(s.clone()));
        next.truncate(beam);
        paths = next;
    }
    paths.retain(|(s, _)| !s.is_empty());
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cheapest(word: &str) -> String {
        enumerate(&build(word), 50)[0].0.clone()
    }

    #[test]
    fn canonical_reading_is_cheapest() {
        assert_eq!(cheapest("ktb"), "كتب");
        assert_eq!(cheapest("gameela"), "جميلة");
        assert_eq!(cheapest("tayyib"), "تيب");
    }

    #[test]
    fn long_vowel_and_shadda_options_exist() {
        let all: Vec<String> = enumerate(&build("3amel"), 200).into_iter().map(|p| p.0).collect();
        assert!(all.contains(&"عامل".to_string()));
        let all: Vec<String> = enumerate(&build("barra"), 200).into_iter().map(|p| p.0).collect();
        assert!(all.contains(&"برة".to_string()));
        assert!(all.contains(&"بررة".to_string()));
    }
}
