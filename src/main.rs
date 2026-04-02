use arabizi::TransliterationEngine;
use std::io::{self, BufRead, Write};

fn main() {
    let engine = TransliterationEngine::new();

    println!("Arabizi Transliterator — type Arabizi text and press Enter");
    println!("Type 'quit' to exit\n");

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        print!("> ");
        stdout.flush().unwrap();

        let mut line = String::new();
        if stdin.lock().read_line(&mut line).unwrap() == 0 {
            break;
        }

        let input = line.trim();
        if input.eq_ignore_ascii_case("quit") {
            break;
        }

        let results = engine.transliterate(input);
        for (i, candidate) in results.iter().enumerate() {
            println!("  {}. {}", i + 1, candidate);
        }
        println!();
    }
}
