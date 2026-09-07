use std::collections::HashSet;
use std::sync::OnceLock;

use crate::game::Board;

// The word-list package is a broad lower-case English word list. Keeping it as
// a checked-in asset makes validation deterministic across local and hosted
// servers, without requiring a dictionary package at runtime.
const WORD_LIST: &str = include_str!("../assets/words.txt");
static WORD_SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
static PREFIX_SET: OnceLock<HashSet<&'static str>> = OnceLock::new();

fn dictionary() -> &'static HashSet<&'static str> {
    WORD_SET.get_or_init(|| {
        WORD_LIST
            .lines()
            .map(str::trim)
            .filter(|word| !word.is_empty())
            .collect()
    })
}

fn prefixes() -> &'static HashSet<&'static str> {
    PREFIX_SET.get_or_init(|| {
        let mut result = HashSet::new();
        for word in dictionary() {
            if word.len() <= 25 && word.bytes().all(|byte| byte.is_ascii_lowercase()) {
                for length in 1..=word.len() {
                    result.insert(&word[..length]);
                }
            }
        }
        result
    })
}

pub fn is_word(word: &str) -> bool {
    let lower = word.to_ascii_lowercase();
    dictionary().contains(lower.as_str())
}

pub fn possible_words(board: &Board) -> Vec<String> {
    let mut found = HashSet::new();
    let mut used = vec![false; board.letters.len()];
    let mut current = String::new();
    for start in 0..board.letters.len() {
        collect_from(board, start, &mut current, &mut used, &mut found);
    }
    let mut words: Vec<_> = found.into_iter().collect();
    words.sort_by(|left, right| left.len().cmp(&right.len()).then_with(|| left.cmp(right)));
    words
}

fn collect_from(
    board: &Board,
    index: usize,
    current: &mut String,
    used: &mut [bool],
    found: &mut HashSet<String>,
) {
    if used[index] {
        return;
    }
    current.push_str(&board.letters[index].to_ascii_lowercase());
    if !prefixes().contains(current.as_str()) {
        current.pop();
        return;
    }
    if current.len() >= 3 && dictionary().contains(current.as_str()) {
        found.insert(current.clone());
    }
    if current.len() < 25 {
        used[index] = true;
        let row = index / board.size;
        let col = index % board.size;
        for row_delta in -1i32..=1 {
            for col_delta in -1i32..=1 {
                if row_delta == 0 && col_delta == 0 {
                    continue;
                }
                let next_row = row as i32 + row_delta;
                let next_col = col as i32 + col_delta;
                if next_row >= 0
                    && next_row < board.size as i32
                    && next_col >= 0
                    && next_col < board.size as i32
                {
                    let next = next_row as usize * board.size + next_col as usize;
                    collect_from(board, next, current, used, found);
                }
            }
        }
        used[index] = false;
    }
    current.pop();
}

#[cfg(test)]
mod tests {
    use super::{is_word, possible_words};
    use crate::game::Board;

    #[test]
    fn recognizes_everyday_words() {
        assert!(is_word("WIFE"));
        assert!(is_word("boggle"));
        assert!(is_word("TEA"));
    }

    #[test]
    fn rejects_gibberish() {
        assert!(!is_word("QXQXQ"));
    }

    #[test]
    fn enumerates_reachable_words_once() {
        let board = Board {
            size: 2,
            letters: vec!["S".into(), "U".into(), "E".into(), "X".into()],
        };
        let words = possible_words(&board);
        assert!(words.iter().any(|word| word == "sue"));
        assert_eq!(words.iter().filter(|word| *word == "sue").count(), 1);
    }
}
