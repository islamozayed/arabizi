use std::collections::HashMap;

/// Arabic word frequency data from OpenSubtitles 2018 corpus (CC-BY-SA-4.0).
/// 300k most frequent words — used to rank transliteration candidates.
const FREQUENCY_DATA: &str = include_str!("../data/ar_300k.txt");

pub struct FrequencyList {
    /// Maps Arabic word → (frequency rank (0 = most common), raw corpus count).
    entries: HashMap<String, (u32, u32)>,
}

impl FrequencyList {
    pub fn load() -> Self {
        let mut entries: HashMap<String, (u32, u32)> = HashMap::with_capacity(300_000);
        for (rank, line) in FREQUENCY_DATA.lines().enumerate() {
            if let Some((word, freq)) = line.rsplit_once(' ') {
                let count = freq.trim().parse::<u32>().unwrap_or(1);
                entries
                    .entry(word.to_string())
                    .and_modify(|e| e.1 += count)
                    .or_insert((rank as u32, count));
            }
        }
        FrequencyList { entries }
    }

    /// Returns the rank of a word (lower = more common). None if not found.
    #[allow(dead_code)] // used by tests; ranking now uses `count`
    pub fn rank(&self, word: &str) -> Option<u32> {
        self.entries.get(word).map(|e| e.0)
    }

    /// Raw corpus count of a word. None if not in the list.
    pub fn count(&self, word: &str) -> Option<u32> {
        self.entries.get(word).map(|e| e.1)
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frequency_list_loads() {
        let fl = FrequencyList::load();
        assert!(fl.entries.len() > 250000, "Expected 250k+ entries, got {}", fl.entries.len());
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
