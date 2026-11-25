use crate::table::{read_u16, read_u32, Table};

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
/// Packed into a sorted table keyed by the tashkeel-stripped base form by
/// `build.rs`; lookups binary-search it without touching the heap.
const TASHKEELA_DATA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/tashkeela.bin"));

/// base form (tashkeel-stripped) → (rank (0 = most common), corpus count
/// summed over variants, every vocalized variant with its count, most
/// frequent first — e.g. أَمْر the noun and أَمَرَ the verb).
pub struct TashkeelaList {
    table: Table,
}

impl TashkeelaList {
    pub fn load() -> Self {
        TashkeelaList { table: Table::new(TASHKEELA_DATA) }
    }

    /// Rank of a base (unvocalized) form. Lower = more common.
    #[allow(dead_code)] // ranking now uses `count`
    pub fn rank(&self, base: &str) -> Option<u32> {
        self.table.get(base).map(|p| read_u32(p, 0))
    }

    /// Corpus count of a base (unvocalized) form.
    pub fn count(&self, base: &str) -> Option<u32> {
        self.table.get(base).map(|p| read_u32(p, 4))
    }

    /// All known vocalized variants of a base form with their counts,
    /// most frequent first. Empty if the base is unknown.
    pub fn variants(&self, base: &str) -> Vec<(&'static str, u32)> {
        let Some(p) = self.table.get(base) else { return Vec::new() };
        let n = read_u16(p, 8) as usize;
        let mut out = Vec::with_capacity(n);
        let mut at = 10;
        for _ in 0..n {
            let (v, c, next) = variant_at(p, at);
            out.push((v, c));
            at = next;
        }
        out
    }

    /// Most-frequent vocalized variant for a base form, if known.
    pub fn vocalized(&self, base: &str) -> Option<&'static str> {
        self.table.get(base).map(|p| variant_at(p, 10).0)
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.table.len()
    }
}

/// Reads one `(u16 len, bytes, u32 count)` variant; returns it and the next offset.
fn variant_at(p: &'static [u8], at: usize) -> (&'static str, u32, usize) {
    let len = read_u16(p, at) as usize;
    let s = std::str::from_utf8(&p[at + 2..at + 2 + len]).expect("variants are UTF-8");
    (s, read_u32(p, at + 2 + len), at + 2 + len + 4)
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
            let base = t.table.key(0);
            assert!(t.vocalized(base).is_some());
        }
    }
}
