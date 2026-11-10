//! CORE-U-073: the compaction marks, pinned. A compaction starts above 85 % of the input capacity and aims at 70 %: the
//! 15 points between are room for the next steps, so one compaction doesn't call for another at the next step
//! (decided on 7 Oct 2026). A change to either is a decision, not an edit; that the mark sits below the threshold
//! is checked where they are defined, at compile time.
use tqa_types::{COMPACT_AT_PERCENT, COMPACT_LOW_WATER_PERCENT};

// TEST: CORE-U-073
#[test]
fn core_u_073_a_compaction_starts_at_85_and_aims_at_70() {
    assert_eq!(COMPACT_AT_PERCENT, 85);
    assert_eq!(COMPACT_LOW_WATER_PERCENT, 70);
}
