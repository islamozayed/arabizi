use crate::table::{read_u32, Table};

/// Arabic word frequency data from OpenSubtitles 2018 corpus (CC-BY-SA-4.0).
/// 300k most frequent words — used to rank transliteration candidates.
/// Source: `data/ar_300k.txt`, packed into a sorted table by `build.rs`.
const FREQUENCY_DATA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ar_300k.bin"));

/// Colloquial (mostly Egyptian) word counts from the ASTD tweets and LABR
/// book-review corpora (both GPL-2.0). Built by `scripts/build_eg_freq.py`.
/// OpenSubtitles and Tashkeela lean formal; this nudges spoken forms up.
/// Source: `data/eg_freq.txt`, packed by `build.rs`.
const DIALECT_DATA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/eg_freq.bin"));

/// Maps Arabic word → (frequency rank (0 = most common), raw corpus count).
/// Lookups binary-search the embedded table; nothing is copied to the heap.
pub struct FrequencyList {
    table: Table,
}

impl FrequencyList {
    pub fn load() -> Self {
        FrequencyList { table: Table::new(FREQUENCY_DATA) }
    }

    /// The colloquial word list (see `DIALECT_DATA`).
    pub fn load_dialect() -> Self {
        FrequencyList { table: Table::new(DIALECT_DATA) }
    }

    /// Returns the rank of a word (lower = more common). None if not found.
    #[allow(dead_code)] // used by tests; ranking now uses `count`
    pub fn rank(&self, word: &str) -> Option<u32> {
        self.table.get(word).map(|p| read_u32(p, 0))
    }

    /// Raw corpus count of a word. None if not in the list.
    pub fn count(&self, word: &str) -> Option<u32> {
        self.table.get(word).map(|p| read_u32(p, 4))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frequency_list_loads() {
        let fl = FrequencyList::load();
        assert!(fl.table.len() > 250000, "Expected 250k+ entries, got {}", fl.table.len());
    }

    #[test]
    fn common_words_ranked() {
        let fl = FrequencyList::load();
        assert!(fl.rank("في").is_some());
        assert!(fl.rank("من").is_some());
        assert!(fl.rank("أنا").is_some());
    }

    #[test]
    fn common_words_rank_higher() {
        let fl = FrequencyList::load();
        let rank_fi = fl.rank("في").unwrap();
        let rank_ana = fl.rank("أنا").unwrap();
        // "في" is more common than "أنا"
        assert!(rank_fi < rank_ana);
    }
}
