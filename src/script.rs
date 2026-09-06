//! Reading the plain-text script into the lines and tokens that will be timed.

use anyhow::{Context, Result};
use std::path::Path;

/// One printable token, kept beside the stripped form two spellings must agree on.
pub struct Token {
    pub raw: String,
    pub key: String,
}

/// One line of the script, holding every token it prints.
pub struct Line {
    pub text: String,
    pub tokens: Vec<Token>,
}

/// Strips a word down to the letters and digits that two spellings should agree on.
pub fn key(word: &str) -> String {
    word.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

impl Line {
    /// The match keys of this line's tokens, skipping tokens that are pure punctuation.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.tokens
            .iter()
            .filter(|token| !token.key.is_empty())
            .map(|token| token.key.as_str())
    }
}

/// Reads a script file into its non-empty lines, each carrying its tokens.
pub fn read(path: &Path) -> Result<Vec<Line>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading script {}", path.display()))?;
    Ok(parse(&text))
}

/// Splits script text into lines of tokens, dropping blank rows.
pub fn parse(text: &str) -> Vec<Line> {
    text.lines()
        .map(str::trim)
        .filter(|row| !row.is_empty())
        .map(|row| Line {
            text: row.to_string(),
            tokens: row
                .split_whitespace()
                .map(|raw| Token { raw: raw.to_string(), key: key(raw) })
                .collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_keeps_only_letters_and_digits() {
        assert_eq!(key("There's"), "theres");
        assert_eq!(key("Route-66,"), "route66");
        assert_eq!(key("—"), "");
    }

    #[test]
    fn parse_drops_blank_rows_and_keeps_punctuation() {
        let lines = parse("  One two,  \n\n\tThree!\n");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "One two,");
        assert_eq!(lines[0].tokens[1].raw, "two,");
        assert_eq!(lines[0].tokens[1].key, "two");
    }

    #[test]
    fn keys_skip_tokens_that_are_pure_punctuation() {
        let lines = parse("Yes — no");
        assert_eq!(lines[0].tokens.len(), 3);
        assert_eq!(lines[0].keys().collect::<Vec<_>>(), ["yes", "no"]);
    }
}
