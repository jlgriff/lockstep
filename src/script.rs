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

/// Splits script text into tokenized lyrics, dropping blank and bracketed section rows.
pub fn parse(text: &str) -> Vec<Line> {
    text.lines()
        .map(str::trim)
        .filter(|row| !row.is_empty() && !(row.starts_with('[') && row.ends_with(']')))
        .map(|row| Line {
            text: row.to_string(),
            tokens: row
                .split_whitespace()
                .flat_map(|word| word.split_inclusive(['—', '–', '-']))
                .map(|raw| Token { raw: raw.to_string(), key: key(raw) })
                .collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Aligns words separated by an em dash independently without altering the printed lyric.
    #[test]
    fn em_dashes_separate_alignment_words() {
        let lines = parse("The servant—whom obey?\nHe turned—the road lay bare");
        assert_eq!(lines[0].text, "The servant—whom obey?");
        assert_eq!(lines[0].keys().collect::<Vec<_>>(), ["the", "servant", "whom", "obey"]);
        assert_eq!(lines[0].tokens[1].raw, "servant—");
        assert_eq!(lines[1].keys().collect::<Vec<_>>(), ["he", "turned", "the", "road", "lay", "bare"]);
    }

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
    fn parse_drops_bracketed_section_rows() {
        let lines = parse(
            "[Verse 1]\nFirst line\n  [Chorus – tenor with choir behind]  \nSecond line\n",
        );
        assert_eq!(
            lines.iter().map(|line| line.text.as_str()).collect::<Vec<_>>(),
            ["First line", "Second line"]
        );
    }

    #[test]
    fn keys_skip_tokens_that_are_pure_punctuation() {
        let lines = parse("Yes — no");
        assert_eq!(lines[0].tokens.len(), 3);
        assert_eq!(lines[0].keys().collect::<Vec<_>>(), ["yes", "no"]);
    }
}
