use std::collections::HashMap;

/// Arabic word frequency data from OpenSubtitles 2018 corpus (CC-BY-SA-4.0).
/// 300k most frequent words — used to rank transliteration candidates.
const FREQUENCY_DATA: &str = include_str!("../data/ar_300k.txt");

pub struct FrequencyList {
    /// Maps Arabic word → frequency rank (0 = most common)
    ranks: HashMap<String, u32>,
}

impl FrequencyList {
    pub fn load() -> Self {
        let mut ranks = HashMap::new();
        for (rank, line) in FREQUENCY_DATA.lines().enumerate() {
            if let Some((word, _freq)) = line.rsplit_once(' ') {
                ranks.insert(word.to_string(), rank as u32);
            }
        }
        FrequencyList { ranks }
    }

    /// Returns the rank of a word (lower = more common). None if not found.
    pub fn rank(&self, word: &str) -> Option<u32> {
        self.ranks.get(word).copied()
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frequency_list_loads() {
        let fl = FrequencyList::load();
        assert!(fl.ranks.len() > 250000, "Expected 250k+ entries, got {}", fl.ranks.len());
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
