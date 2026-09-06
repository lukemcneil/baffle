use std::collections::HashSet;
use std::sync::OnceLock;

// The word-list package is a broad lower-case English word list. Keeping it as
// a checked-in asset makes validation deterministic across local and hosted
// servers, without requiring a dictionary package at runtime.
const WORD_LIST: &str = include_str!("../assets/words.txt");
static WORD_SET: OnceLock<HashSet<&'static str>> = OnceLock::new();

fn dictionary() -> &'static HashSet<&'static str> {
    WORD_SET.get_or_init(|| {
        WORD_LIST
            .lines()
            .map(str::trim)
            .filter(|word| !word.is_empty())
            .collect()
    })
}

pub fn is_word(word: &str) -> bool {
    let lower = word.to_ascii_lowercase();
    dictionary().contains(lower.as_str())
}

#[cfg(test)]
mod tests {
    use super::is_word;

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
}
