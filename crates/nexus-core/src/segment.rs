//! The byte map of a download and the work-stealing split logic.
//!
//! A file starts as a single segment. Each new connection either picks up an
//! idle unfinished segment (after a resume) or halves the segment with the most
//! bytes left, so faster connections keep stealing work from slower ones until
//! every remaining piece is smaller than `min_split`. This is the dynamic
//! segmentation IDM popularised: the number and size of chunks follow the
//! network, not a fixed plan made up front.

use crate::types::SegmentState;

#[derive(Debug, Clone)]
struct Slot {
    state: SegmentState,
    /// A worker is currently fetching this segment.
    owned: bool,
    /// Bytes counted in `state.downloaded` that are still being written.
    in_flight: u64,
}

#[derive(Debug, Clone)]
pub struct SegmentTable {
    slots: Vec<Slot>,
    min_split: u64,
}

impl SegmentTable {
    /// A fresh table covering `[0, total)` as one segment.
    #[must_use]
    pub fn new(total: u64, min_split: u64) -> Self {
        Self::from_states(
            vec![SegmentState {
                start: 0,
                end: total,
                downloaded: 0,
            }],
            min_split,
        )
    }

    /// Rebuilds a table from a persisted byte map.
    #[must_use]
    pub fn from_states(states: Vec<SegmentState>, min_split: u64) -> Self {
        Self {
            slots: states
                .into_iter()
                .map(|state| Slot {
                    state,
                    owned: false,
                    in_flight: 0,
                })
                .collect(),
            min_split: min_split.max(1),
        }
    }

    /// Hands a worker a segment to fetch: an idle unfinished one if any,
    /// otherwise the second half of the largest segment still in progress.
    pub fn acquire(&mut self) -> Option<usize> {
        if let Some(idx) = self
            .slots
            .iter()
            .position(|s| !s.owned && !s.state.is_complete())
        {
            self.slots[idx].owned = true;
            return Some(idx);
        }
        let idx = self.split_largest()?;
        self.slots[idx].owned = true;
        Some(idx)
    }

    /// Whether another connection would get any work right now.
    #[must_use]
    pub fn can_accept_worker(&self) -> bool {
        self.slots.iter().any(|s| {
            (!s.owned && !s.state.is_complete()) || s.state.remaining() >= self.min_split * 2
        })
    }

    fn split_largest(&mut self) -> Option<usize> {
        let (idx, remaining) = self
            .slots
            .iter()
            .enumerate()
            .map(|(i, s)| (i, s.state.remaining()))
            .max_by_key(|&(_, r)| r)?;
        if remaining < self.min_split * 2 {
            return None;
        }
        let donor = &mut self.slots[idx].state;
        let mid = donor.cursor() + remaining / 2;
        let tail = SegmentState {
            start: mid,
            end: donor.end,
            downloaded: 0,
        };
        donor.end = mid;
        self.slots.push(Slot {
            state: tail,
            owned: false,
            in_flight: 0,
        });
        Some(self.slots.len() - 1)
    }

    pub fn release(&mut self, idx: usize) {
        self.slots[idx].owned = false;
    }

    #[must_use]
    pub fn cursor(&self, idx: usize) -> u64 {
        self.slots[idx].state.cursor()
    }

    #[must_use]
    pub fn end(&self, idx: usize) -> u64 {
        self.slots[idx].state.end
    }

    /// Claims up to `len` bytes at the segment cursor for writing. Returns how
    /// many bytes may be written (the segment may have been split meanwhile).
    /// Claimed bytes move the cursor immediately so a concurrent split never
    /// lands inside them, but they only count as flushed after [`Self::commit`].
    pub fn reserve(&mut self, idx: usize, len: u64) -> u64 {
        let slot = &mut self.slots[idx];
        let accepted = len.min(slot.state.remaining());
        slot.state.downloaded += accepted;
        slot.in_flight += accepted;
        accepted
    }

    /// Marks reserved bytes as written to the file.
    pub fn commit(&mut self, idx: usize, len: u64) {
        self.slots[idx].in_flight -= len;
    }

    /// Returns reserved bytes whose write failed.
    pub fn rollback(&mut self, idx: usize, len: u64) {
        let slot = &mut self.slots[idx];
        slot.in_flight -= len;
        slot.state.downloaded -= len;
    }

    /// The byte map as far as it is safely on disk (in-flight writes excluded).
    #[must_use]
    pub fn flushed_states(&self) -> Vec<SegmentState> {
        self.slots
            .iter()
            .map(|s| SegmentState {
                downloaded: s.state.downloaded - s.in_flight,
                ..s.state
            })
            .collect()
    }

    #[must_use]
    pub fn flushed_bytes(&self) -> u64 {
        self.slots
            .iter()
            .map(|s| s.state.downloaded - s.in_flight)
            .sum()
    }

    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.slots
            .iter()
            .all(|s| s.state.is_complete() && s.in_flight == 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: u64 = 1024 * 1024;

    fn covered(table: &SegmentTable, total: u64) -> bool {
        let mut states = table.flushed_states();
        states.sort_by_key(|s| s.start);
        let mut at = 0;
        for s in states {
            if s.start != at {
                return false;
            }
            at = s.end;
        }
        at == total
    }

    #[test]
    fn splits_largest_remaining_in_half() {
        let mut t = SegmentTable::new(100 * MB, MB);
        assert_eq!(t.acquire(), Some(0));
        assert_eq!(t.acquire(), Some(1));
        assert_eq!(t.end(0), 50 * MB);
        assert_eq!(t.cursor(1), 50 * MB);
        assert_eq!(t.acquire(), Some(2));
        assert!(covered(&t, 100 * MB));
    }

    #[test]
    fn split_starts_after_reserved_bytes() {
        let mut t = SegmentTable::new(10 * MB, MB);
        let a = t.acquire().unwrap();
        assert_eq!(t.reserve(a, 4 * MB), 4 * MB);
        let b = t.acquire().unwrap();
        assert_eq!(t.cursor(b), 7 * MB);
        assert_eq!(t.flushed_bytes(), 0);
        t.commit(a, 4 * MB);
        assert_eq!(t.flushed_bytes(), 4 * MB);
        assert!(covered(&t, 10 * MB));
    }

    #[test]
    fn reserve_clamps_to_shrunk_segment() {
        let mut t = SegmentTable::new(4 * MB, MB);
        let a = t.acquire().unwrap();
        let _b = t.acquire().unwrap();
        assert_eq!(t.reserve(a, 3 * MB), 2 * MB);
    }

    #[test]
    fn stops_splitting_below_minimum() {
        let mut t = SegmentTable::new(3 * MB, 2 * MB);
        assert!(t.acquire().is_some());
        assert!(!t.can_accept_worker());
        assert!(t.acquire().is_none());
    }

    #[test]
    fn resumed_idle_segments_are_reused_first() {
        let states = vec![
            SegmentState {
                start: 0,
                end: 10,
                downloaded: 10,
            },
            SegmentState {
                start: 10,
                end: 20,
                downloaded: 3,
            },
        ];
        let mut t = SegmentTable::from_states(states, 1);
        assert_eq!(t.acquire(), Some(1));
        assert_eq!(t.cursor(1), 13);
    }

    #[test]
    fn rollback_restores_cursor() {
        let mut t = SegmentTable::new(10, 1);
        let a = t.acquire().unwrap();
        t.reserve(a, 4);
        t.rollback(a, 4);
        assert_eq!(t.cursor(a), 0);
        assert!(!t.is_complete());
        t.reserve(a, 10);
        t.commit(a, 10);
        assert!(t.is_complete());
    }
}
