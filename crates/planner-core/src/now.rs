//! "What's now / what's next" for a day's blocks.

use crate::model::DayBlock;

#[derive(Clone, Copy, Debug)]
pub struct NowView<'a> {
    pub now_min: u16,
    /// The block running right now, if any.
    pub current: Option<&'a DayBlock>,
    /// The next block to start after the current one (or after now, in a gap).
    pub next: Option<&'a DayBlock>,
}

impl NowView<'_> {
    /// Minutes left in the current block.
    pub fn remaining(&self) -> Option<u16> {
        self.current.map(|c| c.end_min - self.now_min)
    }

    /// Percent of the current block that has passed.
    pub fn progress(&self) -> Option<u8> {
        self.current.map(|c| {
            let len = (c.end_min - c.start_min) as u32;
            (((self.now_min - c.start_min) as u32 * 100) / len) as u8
        })
    }
}

/// `blocks` must be one day's blocks (any order).
pub fn now_view(blocks: &[DayBlock], now_min: u16) -> NowView<'_> {
    let current = blocks.iter().find(|b| b.start_min <= now_min && now_min < b.end_min);
    let after = current.map_or(now_min + 1, |c| c.end_min);
    let next = blocks.iter().filter(|b| b.start_min >= after).min_by_key(|b| b.start_min);
    NowView { now_min, current, next }
}

/// The block a Done/Skip tap applies to: the current block, or in a gap the
/// most recent ended block that still has no answer.
pub fn action_target(blocks: &[DayBlock], now_min: u16) -> Option<&DayBlock> {
    now_view(blocks, now_min).current.or_else(|| {
        blocks
            .iter()
            .filter(|b| b.end_min <= now_min && b.status.is_open())
            .max_by_key(|b| b.end_min)
    })
}
