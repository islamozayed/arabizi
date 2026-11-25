//! Small text helpers shared by the session and the platform UIs.

/// Maps an ASCII punctuation character to its Arabic equivalent.
pub fn to_arabic_punct(c: char) -> char {
    match c {
        '?' => '؟',
        ',' => '،',
        ';' => '؛',
        c => c,
    }
}

/// Strips leading and trailing ASCII punctuation from a word token, returning
/// `(arabic_prefix, core_word, arabic_suffix)`. "ya3ni?" → ("", "ya3ni", "؟").
pub fn extract_punctuation(word: &str) -> (String, String, String) {
    let chars: Vec<char> = word.chars().collect();
    let n = chars.len();
    let lead_n = chars.iter().take_while(|c| c.is_ascii_punctuation()).count();
    let trail_n = chars
        .iter()
        .rev()
        .take_while(|c| c.is_ascii_punctuation())
        .count()
        .min(n.saturating_sub(lead_n));
    let prefix = chars[..lead_n].iter().map(|&c| to_arabic_punct(c)).collect();
    let core = chars[lead_n..n - trail_n].iter().collect();
    let suffix = chars[n - trail_n..].iter().map(|&c| to_arabic_punct(c)).collect();
    (prefix, core, suffix)
}

/// Invisible direction-control characters would otherwise be pasted into the
/// target app and can reorder the text around the cursor.
pub fn strip_bidi_controls(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'))
        .collect()
}

pub fn is_tashkeel(c: char) -> bool {
    matches!(c as u32, 0x064B..=0x0652 | 0x0670 | 0x0610..=0x061A)
}

pub fn strip_tashkeel(s: &str) -> String {
    s.chars().filter(|&c| !is_tashkeel(c)).collect()
}

/// The name of God, written with shadda and dagger alif as it is in print.
pub const ALLAH_VOCALIZED: &str = "اللّٰه";

/// Writes every standalone الله (bare or partly vocalized) with its customary
/// tashkeel, keeping any punctuation around it and the oath prefixes و / ب / ت
/// (والله، بالله، تالله). Other words are untouched.
pub fn vocalize_allah(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    for (i, token) in text.split(' ').enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(&vocalize_token(token));
    }
    out
}

fn vocalize_token(token: &str) -> String {
    // Split off surrounding punctuation (Arabic or ASCII).
    let is_punct = |c: char| c.is_ascii_punctuation() || matches!(c, '؟' | '،' | '؛' | '!' | '.');
    let start = token.find(|c: char| !is_punct(c)).unwrap_or(token.len());
    let end = token.rfind(|c: char| !is_punct(c)).map(|i| i + token[i..].chars().next().unwrap().len_utf8()).unwrap_or(start);
    if start >= end {
        return token.to_string();
    }
    let core = &token[start..end];
    let bare = strip_tashkeel(core);
    let replaced = match bare.as_str() {
        "الله" => Some(ALLAH_VOCALIZED.to_string()),
        "والله" => Some(format!("وَ{ALLAH_VOCALIZED}")),
        "بالله" => Some(format!("بِ{ALLAH_VOCALIZED}")),
        "تالله" => Some(format!("تَ{ALLAH_VOCALIZED}")),
        _ => None,
    };
    match replaced {
        Some(r) => format!("{}{}{}", &token[..start], r, &token[end..]),
        None => token.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn punctuation_split() {
        assert_eq!(extract_punctuation("ya3ni?"), ("".into(), "ya3ni".into(), "؟".into()));
        assert_eq!(extract_punctuation("?!"), ("؟!".into(), "".into(), "".into()));
    }

    #[test]
    fn allah_is_vocalized_only_as_a_whole_word() {
        assert_eq!(vocalize_allah("الله"), "اللّٰه");
        assert_eq!(vocalize_allah("إن شاء الله"), "إن شاء اللّٰه");
        assert_eq!(vocalize_allah("الله؟"), "اللّٰه؟");
        assert_eq!(vocalize_allah("والله"), "وَاللّٰه");
        assert_eq!(vocalize_allah("اللَّه"), "اللّٰه");
        assert_eq!(vocalize_allah("الحمد لله"), "الحمد لله");
        assert_eq!(vocalize_allah("اللهم"), "اللهم");
        assert_eq!(vocalize_allah("عبدالله"), "عبدالله");
    }
}
