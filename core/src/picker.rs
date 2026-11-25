//! The Cmd+G letter picker and Cmd+T tashkeel picker: which Latin letters the
//! user is pointing at, which Arabic options to offer, and how picks are kept
//! across edits. Ported from the web UI.

use crate::text::{is_tashkeel, strip_tashkeel};
use arabizi_engine::LetterSlot;
use std::collections::{BTreeMap, HashMap};

/// Latin char position → chosen Arabic (possibly with tashkeel, or "" when a
/// digraph's second letter is consumed by the first).
pub type Overrides = BTreeMap<usize, String>;

pub fn to_engine(ov: &Overrides) -> HashMap<usize, String> {
    ov.iter().map(|(k, v)| (*k, v.clone())).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Letter,
    Tashkeel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// One or more selected letters.
    Letter,
    /// The whole word, vocalized from the corpus (Cmd+T with the caret at the end).
    Word,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chip {
    pub display: String,
    pub picks: Overrides,
}

#[derive(Debug, Clone)]
pub struct Picker {
    pub mode: Mode,
    pub scope: Scope,
    pub slot_indices: Vec<usize>,
    pub chips: Vec<Chip>,
    pub alt_idx: usize,
}

impl Picker {
    /// Latin letters the picker acts on, shown at its start.
    pub fn label(&self, slots: &[LetterSlot]) -> String {
        self.slot_indices.iter().filter_map(|&i| slots.get(i)).map(|s| s.pattern.as_str()).collect()
    }

    /// False when the chosen letters have no alternatives: the chips are then
    /// shown disabled next to "no alternatives".
    pub fn has_choices(&self, slots: &[LetterSlot]) -> bool {
        let any_alts = self.mode == Mode::Tashkeel
            || self.slot_indices.iter().any(|&i| slots.get(i).is_some_and(|s| !s.alternatives.is_empty()));
        !self.chips.is_empty() && any_alts
    }
}

pub const TASHKEEL_OPTIONS: [(&str, &str); 12] = [
    ("none", ""),
    ("fatha", "\u{064E}"),
    ("kasra", "\u{0650}"),
    ("damma", "\u{064F}"),
    ("sukun", "\u{0652}"),
    ("shadda", "\u{0651}"),
    ("shadda+fatha", "\u{064E}\u{0651}"),
    ("shadda+kasra", "\u{0650}\u{0651}"),
    ("shadda+damma", "\u{064F}\u{0651}"),
    ("tanween fath", "\u{064B}"),
    ("tanween kasr", "\u{064D}"),
    ("tanween damm", "\u{064C}"),
];

/// Digraphs that map a 2- or 3-letter Latin span to one Arabic letter.
pub const DIGRAPHS: [(&str, &[&str]); 8] = [
    ("tch", &["تش"]),
    ("sh", &["ش"]),
    ("ch", &["ش", "تش"]),
    ("kh", &["خ"]),
    ("th", &["ث", "ذ"]),
    ("dh", &["ذ", "ض", "ظ"]),
    ("gh", &["غ"]),
    ("ph", &["ف"]),
];

fn digraph(latin: &str) -> Option<&'static [&'static str]> {
    DIGRAPHS.iter().find(|(k, _)| *k == latin).map(|(_, v)| *v)
}

/// Aligns an Arabic candidate against the word's letter slots and returns
/// the non-primary picks it implies, or `None` if it can't be aligned.
/// Vowel slots the engine dropped are skipped; trailing tashkeel is allowed.
pub fn derive_overrides_from_candidate(slots: &[LetterSlot], arabic: &str) -> Option<Overrides> {
    let a: Vec<char> = arabic.chars().collect();
    let mut out = Overrides::new();
    let mut ai = 0usize;
    let consume_marks = |start: usize| {
        let mut end = start;
        while end < a.len() && is_tashkeel(a[end]) {
            end += 1;
        }
        end
    };
    for slot in slots {
        let mut matched: Option<(String, usize)> = None;
        for opt in std::iter::once(&slot.primary).chain(slot.alternatives.iter()) {
            let o: Vec<char> = opt.chars().collect();
            if !o.is_empty() && a.len() >= ai + o.len() && a[ai..ai + o.len()] == o[..] {
                let end = consume_marks(ai + o.len());
                matched = Some((a[ai..end].iter().collect(), end - ai));
                break;
            }
        }
        if let Some((m, len)) = matched {
            if m != slot.primary {
                out.insert(slot.pos, m);
            }
            ai += len;
            continue;
        }
        if !slot.pattern.is_empty() && slot.pattern.chars().all(|c| "aeiouAEIOU".contains(c)) {
            continue;
        }
        return None;
    }
    if consume_marks(ai) != a.len() {
        return None;
    }
    Some(out)
}

/// Slot indices consumed by a combined digraph override starting at
/// `start`, or `None` when the overrides don't form such a span.
pub fn collapsed_span_slot_indices(slots: &[LetterSlot], start: usize, ov: &Overrides) -> Option<Vec<usize>> {
    let anchor = slots.get(start)?;
    let value = ov.get(&anchor.pos).filter(|v| !v.is_empty())?;
    let target = strip_tashkeel(value);
    let mut latin = anchor.pattern.clone();
    for end in start + 1..slots.len() {
        latin.push_str(&slots[end].pattern);
        if !DIGRAPHS.iter().any(|(k, _)| k.starts_with(&latin)) {
            break;
        }
        let Some(targets) = digraph(&latin) else { continue };
        if !targets.contains(&target.as_str()) {
            continue;
        }
        let mut indices = Vec::new();
        let mut all_consumed = true;
        for i in start..=end {
            indices.push(i);
            if i > start && ov.get(&slots[i].pos).map(|s| s.as_str()) != Some("") {
                all_consumed = false;
                break;
            }
        }
        if all_consumed {
            return Some(indices);
        }
    }
    None
}

/// Keeps only overrides that still sit on a slot offering that letter (the
/// user may have edited the word since picking).
pub fn reconcile(slots: &[LetterSlot], ov: &Overrides) -> Overrides {
    let mut next = Overrides::new();
    let mut consumed = vec![false; slots.len()];
    for i in 0..slots.len() {
        if consumed[i] {
            continue;
        }
        let slot = &slots[i];
        let Some(v) = ov.get(&slot.pos) else { continue };
        if let Some(span) = collapsed_span_slot_indices(slots, i, ov) {
            for si in span {
                let p = slots[si].pos;
                next.insert(p, ov.get(&p).cloned().unwrap_or_default());
                consumed[si] = true;
            }
            continue;
        }
        let base = strip_tashkeel(v);
        if !v.is_empty() && (slot.primary == base || slot.alternatives.contains(&base)) {
            next.insert(slot.pos, v.clone());
        }
    }
    next
}

/// Which slots the caret/selection points at. `raw` is the field text and
/// `sel` its selection in chars; slot positions are relative to the trimmed text.
pub fn slot_indices_from_selection(slots: &[LetterSlot], raw: &str, sel: (usize, usize)) -> Vec<usize> {
    if slots.is_empty() {
        return Vec::new();
    }
    let leading = raw.chars().take_while(|c| c.is_whitespace()).count();
    let start = sel.0.saturating_sub(leading);
    let end = sel.1.saturating_sub(leading).max(start);
    if end > start {
        let overlapping: Vec<usize> = slots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.pos < end && s.pos + s.pattern.chars().count() > start)
            .map(|(i, _)| i)
            .collect();
        if !overlapping.is_empty() {
            return overlapping;
        }
    }
    let mut containing = None;
    let mut preceding = None;
    for (i, s) in slots.iter().enumerate() {
        let send = s.pos + s.pattern.chars().count();
        if s.pos < start && send >= start {
            containing = Some(i);
        }
        if s.pos < start {
            preceding = Some(i);
        }
        if s.pos == start {
            return vec![i];
        }
    }
    vec![containing.or(preceding).unwrap_or(slots.len() - 1)]
}

fn current_base_letter(slot: &LetterSlot, ov: &Overrides) -> String {
    match ov.get(&slot.pos) {
        Some(v) if !v.is_empty() => strip_tashkeel(v),
        _ => slot.primary.clone(),
    }
}

/// Builds the chips for a picker. For tashkeel word scope, `vocalized` is the
/// corpus form of the top candidate; when it can't be used the picker falls
/// back to letter chips on the last consonant, updating `slot_indices`.
pub fn build_chips(
    mode: Mode,
    scope: Scope,
    slot_indices: &mut Vec<usize>,
    slots: &[LetterSlot],
    ov: &Overrides,
    top_candidate: Option<&str>,
    vocalized: impl Fn(&str) -> Option<String>,
) -> Vec<Chip> {
    let mut chosen: Vec<&LetterSlot> = slot_indices.iter().filter_map(|&i| slots.get(i)).collect();
    if chosen.is_empty() {
        return Vec::new();
    }
    if mode == Mode::Tashkeel {
        if scope == Scope::Word {
            if let Some(top) = top_candidate {
                if let Some(v) = vocalized(top).filter(|v| v != top) {
                    if let Some(picks) = derive_overrides_from_candidate(slots, &v) {
                        return vec![Chip { display: v, picks }];
                    }
                }
            }
            let last = slots
                .iter()
                .rev()
                .find(|s| s.pattern.chars().next().is_some_and(|c| !"aeiouAEIOU".contains(c)))
                .unwrap_or(&slots[slots.len() - 1]);
            let idx = slots.iter().position(|s| std::ptr::eq(s, last)).unwrap();
            *slot_indices = vec![idx];
            chosen = vec![last];
        }
        let first = chosen[0];
        let base = current_base_letter(first, ov);
        let tail: String = chosen[1..].iter().map(|s| current_base_letter(s, ov)).collect();
        return TASHKEEL_OPTIONS
            .iter()
            .map(|(_, marks)| Chip {
                display: format!("{base}{marks}{tail}"),
                picks: Overrides::from([(first.pos, format!("{base}{marks}"))]),
            })
            .collect();
    }

    // Letter mode: cartesian product of each slot's options.
    let mut acc = vec![Chip { display: String::new(), picks: Overrides::new() }];
    for s in &chosen {
        let opts: Vec<&String> = std::iter::once(&s.primary).chain(s.alternatives.iter()).collect();
        let mut next = Vec::new();
        for carry in &acc {
            for opt in &opts {
                let mut picks = carry.picks.clone();
                picks.insert(s.pos, (*opt).clone());
                next.push(Chip { display: format!("{}{}", carry.display, opt), picks });
            }
        }
        acc = next;
    }
    if chosen.len() >= 2 {
        let latin: String = chosen.iter().map(|s| s.pattern.as_str()).collect();
        if let Some(alts) = digraph(&latin) {
            let existing: Vec<String> = acc.iter().map(|c| c.display.clone()).collect();
            for arabic in alts.iter().rev() {
                if existing.iter().any(|d| d == arabic) {
                    continue;
                }
                let mut picks = Overrides::from([(chosen[0].pos, arabic.to_string())]);
                for s in &chosen[1..] {
                    picks.insert(s.pos, String::new());
                }
                acc.insert(0, Chip { display: arabic.to_string(), picks });
            }
        }
    }
    acc
}

/// Index of the chip matching the word's current state, for initial focus.
pub fn focused_chip(chips: &[Chip], slots: &[LetterSlot], ov: &Overrides) -> usize {
    chips
        .iter()
        .position(|chip| {
            chip.picks.iter().all(|(pos, val)| {
                let current = ov
                    .get(pos)
                    .filter(|v| !v.is_empty())
                    .cloned()
                    .or_else(|| slots.iter().find(|s| s.pos == *pos).map(|s| s.primary.clone()));
                current.as_deref() == Some(val.as_str())
            })
        })
        .unwrap_or(0)
}
