//! Map each replaced byte range back to the original and forward to the final text.
use std::ops::Range;

#[derive(Clone)]
pub(super) struct Region {
    pub(super) old: Range<usize>,
    pub(super) new: Range<usize>,
}
#[derive(Default)]
pub(super) struct Regions(pub(super) Vec<Region>);
impl Regions {
    pub(super) fn replace(&mut self, matches: &[usize], old_len: usize, new_len: usize) {
        let mut updated: Vec<_> = matches
            .iter()
            .enumerate()
            .map(|(index, &start)| {
                let new_start = start - index * old_len + index * new_len;
                Region {
                    old: self.original(start, false)..self.original(start + old_len, true),
                    new: new_start..new_start + new_len,
                }
            })
            .collect();
        updated.extend(self.0.iter().map(|region| Region {
            old: region.old.clone(),
            new: remap(region.new.start, false, matches, old_len, new_len)
                ..remap(region.new.end, true, matches, old_len, new_len),
        }));
        updated.sort_unstable_by_key(|region| (region.new.start, region.old.start));
        self.0.clear();
        for region in updated {
            if let Some(previous) = self.0.last_mut()
                && (region.new.start <= previous.new.end || region.old.start <= previous.old.end)
            {
                previous.old.end = previous.old.end.max(region.old.end);
                previous.new.end = previous.new.end.max(region.new.end);
            } else {
                self.0.push(region);
            }
        }
    }
    fn original(&self, position: usize, end: bool) -> usize {
        let index = self.0.partition_point(|region| region.new.end < position);
        if let Some(region) = self.0.get(index)
            && region.new.start <= position
        {
            if position == region.new.end {
                return region.old.end;
            }
            return if end && position != region.new.start {
                region.old.end
            } else {
                region.old.start
            };
        }
        index.checked_sub(1).map_or(position, |previous| {
            position - self.0[previous].new.end + self.0[previous].old.end
        })
    }
}
fn remap(position: usize, end: bool, matches: &[usize], old_len: usize, new_len: usize) -> usize {
    let count = matches.partition_point(|&start| start + old_len <= position);
    let offset = position - count * old_len + count * new_len;
    if let Some(&start) = matches.get(count)
        && start < position
    {
        return start - count * old_len + count * new_len + if end { new_len } else { 0 };
    }
    offset
}
