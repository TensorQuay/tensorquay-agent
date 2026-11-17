//! Why an `old_text` didn't match once, said so a model can fix its next call by itself (18.1 item F, the 18.0 Qwen
//! waves): an ambiguous match lists its lines; a missing one names the nearest lines; a second identical refusal shows
//! the file's region around them. Lines are the model's own file, so nothing new is exposed. Never a looser match.

use std::fmt::Write as _;

/// Matching lines listed in an ambiguous refusal.
pub(super) const LISTED: usize = 10;
/// Overlapping matches counted at most, so a refusal costs a bounded number of finds.
pub(super) const COUNTED: usize = 1_000;
/// Lines equally near named in a not-found refusal.
const NEAREST: usize = 3;
/// The longest line looked at or shown in full; a longer shown line is cut.
const LINE_BYTES: usize = 400;
/// How far a line may be from the anchor to be named: 40 % of the anchor's characters.
const NEAR_PERCENT: usize = 40;
/// Edit-distance computations at most per search: the file is under the read cap, but its lines may be many.
const DISTANCE_BUDGET: usize = 20_000;
/// The region a second identical refusal shows: three lines before the first named, at most this many.
const REGION_LINES: usize = 40;

/// How an `old_text` failed to match once.
#[derive(Debug, Eq, PartialEq)]
pub(super) enum Miss {
    /// No exact match.
    NotFound {
        /// The line where a match differing only in whitespace starts.
        whitespace: Option<usize>,
        /// The nearest lines within [`NEAR_PERCENT`], in order, at most [`NEAREST`].
        nearest: Vec<usize>,
    },
    /// More than one match: each match's line (the first [`LISTED`]) and how many there were, up to [`COUNTED`].
    Ambiguous { lines: Vec<usize>, total: usize },
}

impl Miss {
    /// Why an `old_text` absent from `text` is absent.
    pub(super) fn not_found(text: &str, old: &str) -> Self {
        let normalize = |text: &str| {
            text.split('\n')
                .map(str::trim)
                .collect::<Vec<_>>()
                .join("\n")
        };
        let normalized = normalize(text);
        let whitespace = normalized
            .find(&normalize(old))
            .map(|offset| line_of(&normalized, offset));
        let nearest = if whitespace.is_some() {
            Vec::new()
        } else {
            nearest(text, old, Some(NEAR_PERCENT))
        };
        Self::NotFound {
            whitespace,
            nearest,
        }
    }

    /// The refusal's words for `text` (the file as it stood when this edit was tried).
    pub(super) fn refusal(&self, text: &str) -> String {
        let lines: Vec<&str> = text.split('\n').collect();
        let named = |number: usize| format!("line {number}: {}", shown_line(&lines, number));
        match self {
            Self::NotFound {
                whitespace: Some(number),
                ..
            } => format!(
                "not found; a match differing only in whitespace starts at line {number}: {}",
                shown_line(&lines, *number)
            ),
            Self::NotFound { nearest, .. } => match nearest.as_slice() {
                [] => "not found".to_owned(),
                [number] => format!("not found; the nearest line is {}", named(*number)),
                many => format!(
                    "not found; the nearest lines are:\n{}",
                    many.iter()
                        .map(|&number| named(number))
                        .collect::<Vec<_>>()
                        .join("\n")
                ),
            },
            Self::Ambiguous { lines: at, total } => {
                let numbers = at
                    .iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                let more = total.saturating_sub(at.len());
                let mut listed: Vec<usize> = at.clone();
                listed.dedup();
                format!(
                    "ambiguous; matches at lines {numbers}{}\n{}\nSend old_text with enough of its line to match \
                     once, or replace_all: true.",
                    if *total >= COUNTED {
                        format!(" and {more} or more")
                    } else if more > 0 {
                        format!(" and {more} more")
                    } else {
                        String::new()
                    },
                    listed.into_iter().map(named).collect::<Vec<_>>().join("\n")
                )
            }
        }
    }

    /// What a second identical refusal adds: the region of `text` around the lines this miss named (for a not-found
    /// anchor near no line, around the nearest line whatever its distance), numbered as `read_file` numbers lines.
    pub(super) fn region(&self, path: &str, text: &str, old: &str) -> Option<String> {
        let focus = match self {
            Self::Ambiguous { lines, .. } => lines.first().copied(),
            Self::NotFound {
                whitespace: Some(number),
                ..
            } => Some(*number),
            Self::NotFound { nearest: found, .. } => found
                .first()
                .copied()
                .or_else(|| nearest(text, old, None).first().copied()),
        }?;
        let lines: Vec<&str> = text
            .strip_suffix('\n')
            .unwrap_or(text)
            .split('\n')
            .collect();
        let first = focus.saturating_sub(3).max(1);
        // `focus` is a line of `text`, so `first` <= `focus` <= `last`.
        let last = (first + REGION_LINES - 1).min(lines.len());
        let mut region = format!(
            "You sent the same arguments as the call that just failed. Here are lines {first}-{last} of {path}:\n"
        );
        for number in first..=last {
            // Writing to a String cannot fail.
            let _ = writeln!(region, "{number:>6}\t{}", shown_line(&lines, number));
        }
        region.push_str("Read them and send a different old_text.");
        Some(region)
    }
}

/// Line `number` (from 1) as shown: without its carriage return, cut at [`LINE_BYTES`].
fn shown_line(lines: &[&str], number: usize) -> String {
    let line = lines
        .get(number.wrapping_sub(1))
        .copied()
        .unwrap_or_default()
        .trim_end_matches('\r');
    if line.len() <= LINE_BYTES {
        return line.to_owned();
    }
    let mut end = LINE_BYTES;
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &line[..end])
}

/// The line (from 1) holding byte `offset` of `text`.
pub(super) fn line_of(text: &str, offset: usize) -> usize {
    text[..offset].bytes().filter(|&byte| byte == b'\n').count() + 1
}

/// The lines nearest to `old`'s first non-blank line (both trimmed), by edit distance: those at the smallest
/// distance, in order, at most [`NEAREST`]. With `within` (a percentage of the anchor's characters), only lines at
/// most that far; lines over [`LINE_BYTES`], and an anchor over it, are never compared.
pub(super) fn nearest(text: &str, old: &str, within: Option<usize>) -> Vec<usize> {
    let Some(anchor) = old.split('\n').map(str::trim).find(|line| !line.is_empty()) else {
        return Vec::new();
    };
    if anchor.len() > LINE_BYTES {
        return Vec::new();
    }
    let anchor: Vec<char> = anchor.chars().collect();
    let mut limit = within.map_or(usize::MAX, |percent| anchor.len() * percent / 100);
    let mut best: Vec<usize> = Vec::new();
    let mut budget = DISTANCE_BUDGET;
    for (index, line) in text.split('\n').enumerate() {
        let line = line.trim();
        if line.is_empty() || line.len() > LINE_BYTES {
            continue;
        }
        let line: Vec<char> = line.chars().collect();
        if line.len().abs_diff(anchor.len()) > limit || bag_distance(&anchor, &line) > limit {
            continue;
        }
        if budget == 0 {
            break;
        }
        budget -= 1;
        let Some(distance) = distance(&anchor, &line, limit) else {
            continue;
        };
        if distance < limit || best.is_empty() {
            best.clear();
            limit = distance;
        }
        if best.len() < NEAREST {
            best.push(index + 1);
        }
    }
    best
}

/// A lower bound of the edit distance from the characters' counts alone, cheap enough to skip most lines.
fn bag_distance(left: &[char], right: &[char]) -> usize {
    let mut counts = std::collections::HashMap::<char, isize>::new();
    for &character in left {
        *counts.entry(character).or_default() += 1;
    }
    for &character in right {
        *counts.entry(character).or_default() -= 1;
    }
    let (more, fewer) = counts.values().fold((0, 0), |(more, fewer), &count| {
        if count > 0 {
            (more + count.unsigned_abs(), fewer)
        } else {
            (more, fewer + count.unsigned_abs())
        }
    });
    more.max(fewer)
}

/// The Levenshtein distance between `left` and `right` when it is at most `limit`, in a band of that width.
fn distance(left: &[char], right: &[char], limit: usize) -> Option<usize> {
    if left.len().abs_diff(right.len()) > limit {
        return None;
    }
    let width = right.len();
    let mut previous: Vec<usize> = (0..=width).collect();
    let mut current = vec![usize::MAX; width + 1];
    for (row, &character) in left.iter().enumerate() {
        let row = row + 1;
        let from = row.saturating_sub(limit).max(1);
        let to = (row.saturating_add(limit)).min(width);
        current.fill(usize::MAX);
        current[0] = row;
        let mut smallest = current[0];
        for column in from..=to {
            let substitution =
                previous[column - 1].saturating_add(usize::from(character != right[column - 1]));
            let deletion = previous[column].saturating_add(1);
            let insertion = current[column - 1].saturating_add(1);
            current[column] = substitution.min(deletion).min(insertion);
            smallest = smallest.min(current[column]);
        }
        if smallest > limit {
            return None;
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous
        .get(width)
        .copied()
        .filter(|&distance| distance <= limit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distances_within_a_band() {
        let chars = |text: &str| text.chars().collect::<Vec<_>>();
        assert_eq!(distance(&chars("kitten"), &chars("sitting"), 3), Some(3));
        assert_eq!(distance(&chars("kitten"), &chars("sitting"), 2), None);
        assert_eq!(distance(&chars("'./a'"), &chars("\"./a\""), 2), Some(2));
        assert_eq!(distance(&chars(""), &chars("ab"), 2), Some(2));
        assert_eq!(distance(&chars("界界"), &chars("界"), 1), Some(1));
        assert_eq!(bag_distance(&chars("abc"), &chars("cba")), 0);
        assert_eq!(bag_distance(&chars("abc"), &chars("abd")), 1);
    }

    #[test]
    fn the_nearest_lines_are_the_closest_ones_only() {
        let text = "let a = 1;\nlet b = 1;\nlet bb = 22;\n";
        assert_eq!(nearest(text, "let c = 1;", Some(40)), vec![1, 2]);
        assert_eq!(
            nearest(text, "nothing like it at all", Some(40)),
            Vec::<usize>::new()
        );
        // Unbounded, some line is always nearest (ties named together, at most three).
        let unbounded = nearest(text, "nothing like it at all", None);
        assert!((1..=NEAREST).contains(&unbounded.len()), "{unbounded:?}");
        assert_eq!(nearest(text, "\n\n", None), Vec::<usize>::new());
    }

    #[test]
    fn the_bag_bound_counts_both_sides() {
        let chars = |text: &str| text.chars().collect::<Vec<_>>();
        assert_eq!(bag_distance(&chars("aab"), &chars("b")), 2);
        assert_eq!(bag_distance(&chars("b"), &chars("aab")), 2);
    }

    #[test]
    fn a_line_at_the_limit_is_near_and_ties_are_named_up_to_three() {
        // 40 % of a 10-character anchor is 4: a line 4 edits away is near.
        assert_eq!(nearest("abcdefghijklmn\n", "abcdefghij", Some(40)), vec![1]);
        let ties = "let a = 1;\nlet b = 1;\nlet d = 1;\nlet e = 1;\n";
        assert_eq!(nearest(ties, "let c = 1;", Some(40)), vec![1, 2, 3]);
    }

    #[test]
    fn lines_at_the_byte_bound_are_compared_and_longer_ones_are_not() {
        let at = "a".repeat(LINE_BYTES);
        let near = format!("{}b", "a".repeat(LINE_BYTES - 1));
        assert_eq!(nearest(&format!("{near}\n"), &at, Some(40)), vec![1]);
        assert_eq!(nearest(&format!("{at}\n"), &near, Some(40)), vec![1]);
        let over = "a".repeat(LINE_BYTES + 1);
        assert_eq!(
            nearest(&format!("{near}\n"), &over, None),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn the_search_stops_at_its_budget() {
        // Every line is 2 edits away, then one is 1 away: past the budget it is never looked at.
        let mut text = "let yz = 1;\n".repeat(DISTANCE_BUDGET + 1);
        text.push_str("let x = 2;\n");
        assert_eq!(nearest(&text, "let x = 1;", None), vec![1, 2, 3]);
    }

    #[test]
    fn a_long_line_is_cut_on_a_character() {
        let line = format!("{}é{}", "a".repeat(LINE_BYTES - 1), "z".repeat(10));
        assert_eq!(
            shown_line(&[line.as_str()], 1),
            format!("{}…", "a".repeat(LINE_BYTES - 1))
        );
        assert_eq!(shown_line(&["short\r"], 1), "short");
    }

    #[test]
    fn a_one_line_file_shows_its_line() {
        let miss = Miss::Ambiguous {
            lines: vec![1],
            total: 2,
        };
        assert_eq!(
            miss.region("a.rs", "x x\n", "x").as_deref(),
            Some(
                "You sent the same arguments as the call that just failed. Here are lines 1-1 of a.rs:\n     1\tx x\nRead them and send a different old_text."
            )
        );
    }

    // The search's worst case stays short: 20,000 lines of 400 bytes that all pass the cheap bounds (the same letters,
    // in another order), each compared in full. Timed in a release build only.
    #[test]
    #[cfg_attr(debug_assertions, ignore = "timed in a release build")]
    fn the_search_budget_keeps_the_worst_case_under_a_second() {
        let anchor = "ab".repeat(LINE_BYTES / 2);
        let line = format!(
            "{}{}",
            "a".repeat(LINE_BYTES / 2),
            "b".repeat(LINE_BYTES / 2)
        );
        let text = format!("{line}\n").repeat(DISTANCE_BUDGET);
        let started = std::time::Instant::now();
        let found = nearest(&text, &anchor, None);
        let took = started.elapsed();
        assert_eq!(found.len(), NEAREST);
        assert!(took < std::time::Duration::from_secs(1), "{took:?}");
    }
}
