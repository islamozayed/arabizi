//! Top-1 / top-3 ranking benchmark.
//!
//! Reads `data/benchmark.tsv` (franco<TAB>expected[|alt...]) and measures how
//! often the engine's first candidate (what Space auto-commits) is correct.
//! Prints accuracy and every failure; asserts a floor so ranking regressions
//! are caught.
//!
//! Run with output: `cargo test -p arabizi-engine --release --test benchmark -- --nocapture`

use arabizi_engine::TransliterationEngine;

const BENCHMARK: &str = include_str!("../data/benchmark.tsv");

/// Regression floors (percent). Set slightly below the last measured values.
/// Last measured: top-1 93.7%, top-3 97.7% (572 entries).
const TOP1_FLOOR: f64 = 92.0;
const TOP3_FLOOR: f64 = 96.0;

fn strip_tashkeel(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(*c as u32, 0x064B..=0x0652 | 0x0670 | 0x0610..=0x061A))
        .collect()
}

#[test]
fn ranking_benchmark() {
    let engine = TransliterationEngine::new();

    let mut total = 0usize;
    let mut top1 = 0usize;
    let mut top3 = 0usize;
    let mut failures: Vec<String> = Vec::new();
    // Entries after the "Held-out" marker were never used for tuning;
    // report them separately as a generalization check.
    let mut in_heldout = false;
    let (mut h_total, mut h_top1, mut h_top3) = (0usize, 0usize, 0usize);

    for (lineno, line) in BENCHMARK.lines().enumerate() {
        let line = line.trim_end_matches('\r');
        if line.starts_with('#') && line.contains("Held-out") {
            in_heldout = true;
        }
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let (franco, expected) = line
            .split_once('\t')
            .unwrap_or_else(|| panic!("benchmark.tsv:{}: missing TAB", lineno + 1));
        let accepted: Vec<String> = expected.split('|').map(strip_tashkeel).collect();

        let candidates = engine.transliterate_word_ranked(franco.trim(), None);
        let stripped: Vec<String> = candidates.iter().map(|c| strip_tashkeel(c)).collect();
        let pos = stripped.iter().position(|c| accepted.contains(c));

        total += 1;
        if in_heldout {
            h_total += 1;
            h_top1 += usize::from(pos == Some(0));
            h_top3 += usize::from(pos.is_some_and(|p| p < 3));
        }
        match pos {
            Some(0) => {
                top1 += 1;
                top3 += 1;
            }
            Some(p) => {
                if p < 3 {
                    top3 += 1;
                }
                failures.push(format!(
                    "  {franco:<14} want {expected:<22} rank {} got {:?}",
                    p + 1,
                    &candidates[..candidates.len().min(3)]
                ));
            }
            None => failures.push(format!(
                "  {franco:<14} want {expected:<22} MISSING got {:?}",
                &candidates[..candidates.len().min(3)]
            )),
        }
    }

    let pct = |n: usize| 100.0 * n as f64 / total as f64;
    println!("\n=== Arabizi ranking benchmark ({total} entries) ===");
    println!("top-1: {top1}/{total} = {:.1}%", pct(top1));
    println!("top-3: {top3}/{total} = {:.1}%", pct(top3));
    if h_total > 0 {
        let h = |n: usize| 100.0 * n as f64 / h_total as f64;
        println!(
            "held-out subset ({h_total}): top-1 {:.1}%, top-3 {:.1}%",
            h(h_top1),
            h(h_top3)
        );
    }
    println!("failures ({}):", failures.len());
    for f in &failures {
        println!("{f}");
    }

    assert!(pct(top1) >= TOP1_FLOOR, "top-1 {:.1}% below floor {TOP1_FLOOR}%", pct(top1));
    assert!(pct(top3) >= TOP3_FLOOR, "top-3 {:.1}% below floor {TOP3_FLOOR}%", pct(top3));
}
