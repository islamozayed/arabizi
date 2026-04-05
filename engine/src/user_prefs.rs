use std::collections::HashMap;

/// Tracks which Arabic candidates the user has selected for each Arabizi input.
/// Maps: arabizi_input → { arabic_candidate → selection_count }
///
/// This allows the engine to boost previously chosen translations to the top.
#[derive(Default, Clone)]
pub struct UserPreferences {
    selections: HashMap<String, HashMap<String, u32>>,
}

impl UserPreferences {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that the user chose `arabic` for `input`.
    pub fn record(&mut self, input: &str, arabic: &str) {
        let input = input.to_lowercase();
        let entry = self.selections.entry(input).or_default();
        *entry.entry(arabic.to_string()).or_insert(0) += 1;
    }

    /// Get the selection count for a specific input→arabic pair.
    pub fn score(&self, input: &str, arabic: &str) -> u32 {
        self.selections
            .get(&input.to_lowercase())
            .and_then(|m| m.get(arabic))
            .copied()
            .unwrap_or(0)
    }

    /// Serialize to JSON string for persistence.
    pub fn to_json(&self) -> String {
        let mut out = String::from("{");
        let mut first_outer = true;
        for (input, candidates) in &self.selections {
            if !first_outer { out.push(','); }
            first_outer = false;
            out.push('"');
            out.push_str(&json_escape(input));
            out.push_str("\":{");
            let mut first_inner = true;
            for (arabic, count) in candidates {
                if !first_inner { out.push(','); }
                first_inner = false;
                out.push('"');
                out.push_str(&json_escape(arabic));
                out.push_str("\":");
                out.push_str(&count.to_string());
            }
            out.push('}');
        }
        out.push('}');
        out
    }

    /// Deserialize from JSON string.
    pub fn from_json(json: &str) -> Self {
        let mut prefs = Self::new();
        // Simple JSON parser for our known structure: {"input":{"arabic":count,...},...}
        let json = json.trim();
        if json.len() < 2 || !json.starts_with('{') {
            return prefs;
        }

        // Use a basic state machine to parse
        if let Ok(parsed) = parse_nested_map(json) {
            prefs.selections = parsed;
        }
        prefs
    }
}

fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

/// Parse {"key":{"key2":num,...},...} — minimal JSON parser for our specific format.
fn parse_nested_map(json: &str) -> Result<HashMap<String, HashMap<String, u32>>, ()> {
    let bytes = json.as_bytes();
    let len = bytes.len();
    let mut pos = 0;
    let mut result = HashMap::new();

    // Skip opening {
    pos = skip_ws(bytes, pos);
    if pos >= len || bytes[pos] != b'{' { return Err(()); }
    pos += 1;

    loop {
        pos = skip_ws(bytes, pos);
        if pos >= len { return Err(()); }
        if bytes[pos] == b'}' { break; }
        if bytes[pos] == b',' { pos += 1; pos = skip_ws(bytes, pos); }

        // Parse outer key
        let (key, new_pos) = parse_string(bytes, pos)?;
        pos = skip_ws(bytes, new_pos);
        if pos >= len || bytes[pos] != b':' { return Err(()); }
        pos += 1;

        // Parse inner map
        pos = skip_ws(bytes, pos);
        if pos >= len || bytes[pos] != b'{' { return Err(()); }
        pos += 1;

        let mut inner = HashMap::new();
        loop {
            pos = skip_ws(bytes, pos);
            if pos >= len { return Err(()); }
            if bytes[pos] == b'}' { pos += 1; break; }
            if bytes[pos] == b',' { pos += 1; pos = skip_ws(bytes, pos); }

            let (ikey, new_pos) = parse_string(bytes, pos)?;
            pos = skip_ws(bytes, new_pos);
            if pos >= len || bytes[pos] != b':' { return Err(()); }
            pos += 1;
            pos = skip_ws(bytes, pos);

            let (num, new_pos) = parse_number(bytes, pos)?;
            pos = new_pos;
            inner.insert(ikey, num);
        }
        result.insert(key, inner);
    }

    Ok(result)
}

fn skip_ws(bytes: &[u8], mut pos: usize) -> usize {
    while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
        pos += 1;
    }
    pos
}

fn parse_string(bytes: &[u8], mut pos: usize) -> Result<(String, usize), ()> {
    if pos >= bytes.len() || bytes[pos] != b'"' { return Err(()); }
    pos += 1;
    let mut s = Vec::new();
    while pos < bytes.len() {
        if bytes[pos] == b'\\' && pos + 1 < bytes.len() {
            match bytes[pos + 1] {
                b'"' => { s.push(b'"'); pos += 2; }
                b'\\' => { s.push(b'\\'); pos += 2; }
                b'n' => { s.push(b'\n'); pos += 2; }
                b'r' => { s.push(b'\r'); pos += 2; }
                b't' => { s.push(b'\t'); pos += 2; }
                _ => { s.push(bytes[pos]); pos += 1; }
            }
        } else if bytes[pos] == b'"' {
            pos += 1;
            return String::from_utf8(s).map(|st| (st, pos)).map_err(|_| ());
        } else {
            s.push(bytes[pos]);
            pos += 1;
        }
    }
    Err(())
}

fn parse_number(bytes: &[u8], mut pos: usize) -> Result<(u32, usize), ()> {
    let start = pos;
    while pos < bytes.len() && bytes[pos].is_ascii_digit() {
        pos += 1;
    }
    if pos == start { return Err(()); }
    let s = std::str::from_utf8(&bytes[start..pos]).map_err(|_| ())?;
    s.parse::<u32>().map(|n| (n, pos)).map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_and_score() {
        let mut prefs = UserPreferences::new();
        prefs.record("ahlan", "أهلاً");
        prefs.record("ahlan", "أهلاً");
        prefs.record("ahlan", "اهلان");
        assert_eq!(prefs.score("ahlan", "أهلاً"), 2);
        assert_eq!(prefs.score("ahlan", "اهلان"), 1);
        assert_eq!(prefs.score("ahlan", "nonexistent"), 0);
    }

    #[test]
    fn json_roundtrip() {
        let mut prefs = UserPreferences::new();
        prefs.record("shukran", "شكرًا");
        prefs.record("shukran", "شكرًا");
        prefs.record("ahlan", "أهلاً");

        let json = prefs.to_json();
        let loaded = UserPreferences::from_json(&json);
        assert_eq!(loaded.score("shukran", "شكرًا"), 2);
        assert_eq!(loaded.score("ahlan", "أهلاً"), 1);
    }

    #[test]
    fn empty_json() {
        let prefs = UserPreferences::from_json("");
        assert_eq!(prefs.score("anything", "anything"), 0);

        let prefs = UserPreferences::from_json("{}");
        assert_eq!(prefs.score("anything", "anything"), 0);
    }
}
