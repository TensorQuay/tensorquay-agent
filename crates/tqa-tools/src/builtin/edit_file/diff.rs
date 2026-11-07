//! Bounded unified hunks describe changed regions, without edit-distance matching.
use super::regions::{Region, Regions};
use std::ops::Range;

struct Lines<'a> {
    text: Vec<&'a str>,
    starts: Vec<usize>,
    bytes: usize,
}
impl<'a> Lines<'a> {
    fn new(text: &'a str) -> Self {
        let text: Vec<_> = text.split_inclusive('\n').collect();
        let mut offset = 0;
        let starts = text
            .iter()
            .map(|line| {
                let start = offset;
                offset += line.len();
                start
            })
            .collect();
        Self {
            text,
            starts,
            bytes: offset,
        }
    }
    fn range(&self, range: &Range<usize>) -> Range<usize> {
        let mut start = self.starts.partition_point(|&offset| offset < range.start);
        let boundary =
            range.start == self.bytes && self.text.last().is_none_or(|line| line.ends_with('\n'));
        if !boundary && self.starts.get(start) != Some(&range.start) {
            start = start.saturating_sub(1);
        }
        let end = self.starts.partition_point(|&offset| offset < range.end);
        start..end.max(start)
    }
}
pub(super) fn render(
    path: &str,
    edits: usize,
    before: &str,
    after: &str,
    regions: Regions,
) -> String {
    let old = Lines::new(before);
    let new = Lines::new(after);
    let hunks = hunks(&old, &new, regions);
    let added: usize = hunks.iter().map(|hunk| hunk.new.len()).sum();
    let removed: usize = hunks.iter().map(|hunk| hunk.old.len()).sum();
    let mut diff = Vec::new();
    push(&mut diff, format!("--- a/{path}"));
    push(&mut diff, format!("+++ b/{path}"));
    for hunk in hunks {
        let prefix = hunk.old.start.min(hunk.new.start).min(3);
        let suffix = (old.text.len() - hunk.old.end)
            .min(new.text.len() - hunk.new.end)
            .min(3);
        let old_start = hunk.old.start - prefix;
        let new_start = hunk.new.start - prefix;
        let old_len = hunk.old.len() + prefix + suffix;
        let new_len = hunk.new.len() + prefix + suffix;
        push(
            &mut diff,
            format!(
                "@@ -{},{} +{},{} @@",
                old_start + usize::from(old_len != 0),
                old_len,
                new_start + usize::from(new_len != 0),
                new_len
            ),
        );
        lines(&mut diff, ' ', &old.text[old_start..hunk.old.start]);
        lines(&mut diff, '-', &old.text[hunk.old.clone()]);
        lines(&mut diff, '+', &new.text[hunk.new.clone()]);
        lines(
            &mut diff,
            ' ',
            &old.text[hunk.old.end..hunk.old.end + suffix],
        );
    }
    if diff.len() > 200 {
        diff.truncate(199);
        diff.push("[diff truncated]".into());
    }
    format!(
        "edited {path}: {edits} edits, +{added} -{removed} lines\n{}",
        diff.join("\n")
    )
}
fn hunks(old: &Lines<'_>, new: &Lines<'_>, regions: Regions) -> Vec<Region> {
    let mut hunks: Vec<Region> = Vec::new();
    for region in regions.0 {
        let mut hunk = Region {
            old: old.range(&region.old),
            new: new.range(&region.new),
        };
        while !hunk.old.is_empty()
            && !hunk.new.is_empty()
            && old.text[hunk.old.start] == new.text[hunk.new.start]
        {
            hunk.old.start += 1;
            hunk.new.start += 1;
        }
        while !hunk.old.is_empty()
            && !hunk.new.is_empty()
            && old.text[hunk.old.end - 1] == new.text[hunk.new.end - 1]
        {
            hunk.old.end -= 1;
            hunk.new.end -= 1;
        }
        if hunk.old.is_empty() && hunk.new.is_empty() {
            continue;
        }
        if let Some(previous) = hunks.last_mut()
            && hunk.old.start <= previous.old.end.saturating_add(6)
        {
            previous.old.end = previous.old.end.max(hunk.old.end);
            previous.new.end = previous.new.end.max(hunk.new.end);
        } else {
            hunks.push(hunk);
        }
    }
    hunks
}
fn push(diff: &mut Vec<String>, line: String) {
    if diff.len() <= 200 {
        diff.push(line);
    }
}
fn lines(diff: &mut Vec<String>, prefix: char, lines: &[&str]) {
    for line in lines.iter().take(201_usize.saturating_sub(diff.len())) {
        push(
            diff,
            format!("{prefix}{}", line.trim_end_matches(['\r', '\n'])),
        );
        if !line.ends_with('\n') {
            push(diff, "\\ No newline at end of file".into());
        }
    }
}
