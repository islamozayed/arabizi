//! Packs the corpus word lists into sorted binary tables at compile time.
//!
//! Parsing the text files into `HashMap<String, _>` at startup cost ~160 MB of
//! heap (one allocation per key for ~1M words). Instead we sort the entries
//! here and embed the result with `include_bytes!`: lookups binary-search
//! read-only pages of the executable, which the OS maps lazily and can evict,
//! so they never count as dirty memory.
//!
//! Table layout (all integers little-endian), shared with `src/table.rs`:
//!   u32 n | u32 offsets[n] (record starts, sorted by key bytes) | records
//!   record = u16 key_len | key bytes | payload

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let out = env::var("OUT_DIR").unwrap();
    let out = Path::new(&out);
    for f in ["ar_300k.txt", "eg_freq.txt", "tashkeela_vocalized.txt"] {
        println!("cargo:rerun-if-changed=data/{f}");
    }
    println!("cargo:rerun-if-changed=build.rs");

    fs::write(out.join("ar_300k.bin"), frequency_table(&read("data/ar_300k.txt"))).unwrap();
    fs::write(out.join("eg_freq.bin"), frequency_table(&read("data/eg_freq.txt"))).unwrap();
    fs::write(out.join("tashkeela.bin"), tashkeela_table(&read("data/tashkeela_vocalized.txt"))).unwrap();
}

fn read(path: &str) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {path}: {e}"))
}

/// `<word> <count>` per line, most frequent first. Payload: u32 rank, u32 count.
/// Duplicate words keep the first rank and sum their counts.
fn frequency_table(data: &str) -> Vec<u8> {
    let mut order: Vec<&str> = Vec::new();
    let mut map: HashMap<&str, (u32, u32)> = HashMap::new();
    for (rank, line) in data.lines().enumerate() {
        if let Some((word, freq)) = line.rsplit_once(' ') {
            let count = freq.trim().parse::<u32>().unwrap_or(1);
            map.entry(word)
                .and_modify(|e| e.1 += count)
                .or_insert_with(|| {
                    order.push(word);
                    (rank as u32, count)
                });
        }
    }
    let records = order.into_iter().map(|w| {
        let (rank, count) = map[w];
        let mut p = Vec::with_capacity(8);
        p.extend_from_slice(&rank.to_le_bytes());
        p.extend_from_slice(&count.to_le_bytes());
        (w.to_string(), p)
    });
    pack(records.collect())
}

/// `<vocalized>\t<count>` per line, most frequent first. Keyed by the
/// tashkeel-stripped base. Payload: u32 rank, u32 summed count, u16 n, then
/// n × (u16 len, vocalized bytes, u32 count), most frequent first.
fn tashkeela_table(data: &str) -> Vec<u8> {
    let mut order: Vec<String> = Vec::new();
    let mut map: HashMap<String, (u32, u32, Vec<(&str, u32)>)> = HashMap::new();
    for (rank, line) in data.lines().enumerate() {
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
        match map.get_mut(&base) {
            Some(e) => {
                e.1 += count;
                e.2.push((vocal, count));
            }
            None => {
                order.push(base.clone());
                map.insert(base, (rank as u32, count, vec![(vocal, count)]));
            }
        }
    }
    let records = order.into_iter().map(|base| {
        let (rank, count, variants) = &map[&base];
        let mut p = Vec::new();
        p.extend_from_slice(&rank.to_le_bytes());
        p.extend_from_slice(&count.to_le_bytes());
        p.extend_from_slice(&(variants.len() as u16).to_le_bytes());
        for (v, c) in variants {
            p.extend_from_slice(&(v.len() as u16).to_le_bytes());
            p.extend_from_slice(v.as_bytes());
            p.extend_from_slice(&c.to_le_bytes());
        }
        (base, p)
    });
    pack(records.collect())
}

fn pack(mut records: Vec<(String, Vec<u8>)>) -> Vec<u8> {
    records.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    let n = records.len();
    let mut out = Vec::new();
    out.extend_from_slice(&(n as u32).to_le_bytes());
    let index_at = out.len();
    out.resize(index_at + 4 * n, 0);
    for (i, (key, payload)) in records.iter().enumerate() {
        let off = out.len() as u32;
        out[index_at + 4 * i..index_at + 4 * i + 4].copy_from_slice(&off.to_le_bytes());
        out.extend_from_slice(&(key.len() as u16).to_le_bytes());
        out.extend_from_slice(key.as_bytes());
        out.extend_from_slice(payload);
    }
    out
}

/// Must match `strip_tashkeel` in `src/engine.rs`.
fn strip_tashkeel(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(*c as u32, 0x064B..=0x0652 | 0x0670 | 0x0610..=0x061A))
        .collect()
}
