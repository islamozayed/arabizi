use std::collections::HashMap;

/// Vocalized Arabic word list extracted from the Tashkeela corpus (Taha
/// Zerrouki, GPL). One vocalized form per line, `<form>\t<count>`, sorted by
/// descending corpus frequency. Build with `engine/scripts/build_tashkeela.py`.
///
/// Serves two roles:
///   1. Ranking signal — a candidate whose tashkeel-stripped form matches a
///      base in this list is almost certainly a real MSA/classical word.
///   2. Tashkeel lookup — when the user invokes the tashkeel modifier, we can
///      surface the corpus's most-frequent vocalized form for the candidate.
///
/// The file is optional: if it's empty (placeholder before the pipeline runs)
/// the engine falls back to behaviour identical to before this module existed.
const TASHKEELA_DATA: &str = include_str!("../data/tashkeela_vocalized.txt");

pub struct TashkeelaList {
    /// base form (tashkeel-stripped) → (rank (0 = most common), corpus count
    /// summed over variants, best vocalized variant from the corpus).
    entries: HashMap<String, (u32, u32, &'static str)>,
}

impl TashkeelaList {
    pub fn load() -> Self {
        let mut entries: HashMap<String, (u32, u32, &'static str)> = HashMap::new();
        for (rank, line) in TASHKEELA_DATA.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let (vocal, count) = match line.split_once('\t') {
                Some((v, c)) => (v, c.trim().parse::<u32>().unwrap_or(1)),
                None => (line, 1),
            };
            let base = strip_tashkeel(vocal);
            if base.is_empty() {
                continue;
            }
            // First occurrence wins — file is pre-sorted by descending freq,
            // so the most-frequent vocalized variant is what we keep.
            entries
                .entry(base)
                .and_modify(|e| e.1 += count)
                .or_insert((rank as u32, count, vocal));
        }
        TashkeelaList { entries }
    }

    /// Rank of a base (unvocalized) form. Lower = more common.
    #[allow(dead_code)] // ranking now uses `count`
    pub fn rank(&self, base: &str) -> Option<u32> {
        self.entries.get(base).map(|e| e.0)
    }

    /// Corpus count of a base (unvocalized) form.
    pub fn count(&self, base: &str) -> Option<u32> {
        self.entries.get(base).map(|e| e.1)
    }

    /// Most-frequent vocalized variant for a base form, if known.
    pub fn vocalized(&self, base: &str) -> Option<&str> {
        self.entries.get(base).map(|e| e.2)
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

fn strip_tashkeel(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(*c as u32, 0x064B..=0x0652 | 0x0670 | 0x0610..=0x061A))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_without_panic() {
        let _t = TashkeelaList::load();
    }

    #[test]
    fn stripped_lookup_when_populated() {
        let t = TashkeelaList::load();
        // The list may be empty (placeholder) during early bring-up; only
        // assert real behaviour once it has entries.
        if t.len() > 0 {
            // Pick any base in the map and verify round-trip.
            let (base, _) = t.entries.iter().next().unwrap();
            assert!(t.vocalized(base).is_some());
        }
    }
}
