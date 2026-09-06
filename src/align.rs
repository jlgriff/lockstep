//! Needleman-Wunsch alignment of the script's words against the words whisper heard.

use anyhow::{bail, Result};

const MATCH: i32 = 3;
const MISMATCH: i32 = -2;
const GAP: i32 = -2;

/// Ceiling on the score matrix, past which the exact alignment would need unreasonable memory.
const MAX_CELLS: usize = 100_000_000;

/// Score for pairing two words, which agree only when their match keys are identical.
fn pair(a: &str, b: &str) -> i32 {
    if a == b {
        MATCH
    } else {
        MISMATCH
    }
}

/// For each script word, the index of the heard word it matched, or none if it was never heard.
pub fn align(script: &[&str], heard: &[&str]) -> Result<Vec<Option<usize>>> {
    let (n, m) = (script.len(), heard.len());
    if n.saturating_mul(m) > MAX_CELLS {
        bail!(
            "script ({n} words) against transcript ({m} words) is too large to align in one pass; \
             split the recording and its script into sections"
        );
    }

    let width = m + 1;
    let mut score = vec![0i32; (n + 1) * width];
    let at = |i: usize, j: usize| i * width + j;

    for i in 1..=n {
        score[at(i, 0)] = i as i32 * GAP;
    }
    for j in 1..=m {
        score[at(0, j)] = j as i32 * GAP;
    }
    for i in 1..=n {
        for j in 1..=m {
            score[at(i, j)] = (score[at(i - 1, j - 1)] + pair(script[i - 1], heard[j - 1]))
                .max(score[at(i - 1, j)] + GAP)
                .max(score[at(i, j - 1)] + GAP);
        }
    }

    let mut matched = vec![None; n];
    let (mut i, mut j) = (n, m);
    while i > 0 && j > 0 {
        let paired = pair(script[i - 1], heard[j - 1]);
        if score[at(i, j)] == score[at(i - 1, j - 1)] + paired {
            if paired == MATCH {
                matched[i - 1] = Some(j - 1);
            }
            i -= 1;
            j -= 1;
        } else if score[at(i, j)] == score[at(i - 1, j)] + GAP {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    Ok(matched)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_sequences_match_position_for_position() {
        let words = ["a", "b", "c"];
        assert_eq!(align(&words, &words).unwrap(), [Some(0), Some(1), Some(2)]);
    }

    #[test]
    fn a_misheard_word_costs_only_itself() {
        let script = ["one", "two", "three"];
        let heard = ["one", "too", "three"];
        assert_eq!(align(&script, &heard).unwrap(), [Some(0), None, Some(2)]);
    }

    #[test]
    fn words_after_an_extra_heard_word_still_line_up() {
        let script = ["one", "two"];
        let heard = ["one", "uh", "two"];
        assert_eq!(align(&script, &heard).unwrap(), [Some(0), Some(2)]);
    }

    #[test]
    fn a_line_never_heard_matches_nothing() {
        assert_eq!(align(&["one", "two"], &[]).unwrap(), [None, None]);
        assert!(align(&[], &["one"]).unwrap().is_empty());
    }
}
