//! Justification logic for stack layout.

use std::borrow::Cow;

use crate::style::Justify;

pub(crate) fn justify_offset(offset: u16) -> i16 {
    offset.min(i16::MAX as u16) as i16
}

pub(crate) fn distribute_space(space: u16, slots: usize) -> Vec<u16> {
    if slots == 0 {
        return Vec::new();
    }
    let slots_u16 = slots.min(u16::MAX as usize) as u16;
    let base = space / slots_u16;
    let remainder = space % slots_u16;
    let mut out = vec![base; slots];
    for slot in out.iter_mut().take(remainder as usize) {
        *slot = slot.saturating_add(1);
    }
    out
}

pub(crate) fn apply_justify<'a>(
    justify: Justify,
    available: u16,
    sizes: &[u16],
    gaps: &'a [u16],
    join_overlaps: &[bool],
) -> (i16, Cow<'a, [u16]>) {
    let count = sizes.len();
    if count == 0 {
        return (0, Cow::Borrowed(gaps));
    }

    let mut total = 0u16;
    for size in sizes {
        total = total.saturating_add(*size);
    }
    for gap in gaps {
        total = total.saturating_add(*gap);
    }
    let join_count: u16 = join_overlaps.iter().filter(|&&j| j).count() as u16;
    total = total.saturating_sub(join_count);
    if total >= available {
        return (0, Cow::Borrowed(gaps));
    }
    let leftover = available - total;

    match justify {
        Justify::Start => (0, Cow::Borrowed(gaps)),
        Justify::Center => (justify_offset(leftover / 2), Cow::Borrowed(gaps)),
        Justify::End => (justify_offset(leftover), Cow::Borrowed(gaps)),
        Justify::SpaceBetween => {
            if count < 2 {
                return (0, Cow::Borrowed(gaps));
            }
            let mut gaps = gaps.to_vec();
            let extras = distribute_space(leftover, count - 1);
            for (gap, extra) in gaps.iter_mut().zip(extras) {
                *gap = gap.saturating_add(extra);
            }
            (0, Cow::Owned(gaps))
        }
        Justify::SpaceEvenly => {
            let mut gaps = gaps.to_vec();
            let spaces = distribute_space(leftover, count + 1);
            let offset = spaces.first().copied().unwrap_or(0);
            for (idx, gap) in gaps.iter_mut().enumerate() {
                if let Some(extra) = spaces.get(idx + 1) {
                    *gap = gap.saturating_add(*extra);
                }
            }
            (justify_offset(offset), Cow::Owned(gaps))
        }
        Justify::SpaceAround => {
            let mut gaps = gaps.to_vec();
            let spaces = distribute_space(leftover, count.saturating_mul(2));
            let offset = spaces.first().copied().unwrap_or(0);
            for (idx, gap) in gaps.iter_mut().enumerate() {
                let left = spaces.get(idx.saturating_mul(2) + 1).copied().unwrap_or(0);
                let right = spaces.get(idx.saturating_mul(2) + 2).copied().unwrap_or(0);
                *gap = gap.saturating_add(left.saturating_add(right));
            }
            (justify_offset(offset), Cow::Owned(gaps))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: 3 children of sizes [5, 10, 5], gaps [0, 0], no join overlaps.
    /// Total natural = 20.
    fn call(justify: Justify, leftover: u16) -> (i16, Vec<u16>) {
        let sizes: Vec<u16> = vec![5, 10, 5];
        let gaps: Vec<u16> = vec![0, 0];
        let join_overlaps = vec![false, false];
        let total: u16 = sizes.iter().sum::<u16>() + gaps.iter().sum::<u16>();
        let available = total + leftover;
        let (offset, gaps) = apply_justify(justify, available, &sizes, &gaps, &join_overlaps);
        (offset, gaps.into_owned())
    }

    #[test]
    fn start_no_offset() {
        let (offset, gaps) = call(Justify::Start, 10);
        assert_eq!(offset, 0);
        assert_eq!(gaps, vec![0, 0]);
    }

    #[test]
    fn center_even_leftover() {
        let (offset, gaps) = call(Justify::Center, 10);
        assert_eq!(offset, 5);
        assert_eq!(gaps, vec![0, 0]);
    }

    #[test]
    fn center_odd_leftover() {
        let (offset, gaps) = call(Justify::Center, 11);
        // 11 / 2 = 5 (integer division)
        assert_eq!(offset, 5);
        assert_eq!(gaps, vec![0, 0]);
    }

    #[test]
    fn end_full_offset() {
        let (offset, gaps) = call(Justify::End, 15);
        assert_eq!(offset, 15);
        assert_eq!(gaps, vec![0, 0]);
    }

    #[test]
    fn space_between_zero_children() {
        let (offset, gaps) = apply_justify(Justify::SpaceBetween, 100, &[], &[], &[]);
        assert_eq!(offset, 0);
        assert!(gaps.is_empty());
    }

    #[test]
    fn space_between_one_child() {
        let (offset, gaps) = apply_justify(Justify::SpaceBetween, 50, &[10], &[], &[]);
        assert_eq!(offset, 0);
        assert!(gaps.is_empty());
    }

    #[test]
    fn space_between_two_children() {
        // sizes=[5,5], gaps=[0], total=10, available=20, leftover=10
        let (offset, gaps) = apply_justify(Justify::SpaceBetween, 20, &[5, 5], &[0], &[false]);
        assert_eq!(offset, 0);
        // 1 gap receives all 10
        assert_eq!(gaps, vec![10]);
    }

    #[test]
    fn space_between_three_children() {
        // leftover=11, 2 gaps: distribute_space(11, 2) => base=5, rem=1 → [6, 5]
        let (offset, gaps) = call(Justify::SpaceBetween, 11);
        assert_eq!(offset, 0);
        assert_eq!(gaps, vec![6, 5]);
    }

    #[test]
    fn space_evenly_distributes_into_n_plus_one_slots() {
        // 2 children, leftover=9, slots=3: distribute_space(9,3) = [3,3,3]
        // offset=3, gap[0] += 3
        let (offset, gaps) = apply_justify(Justify::SpaceEvenly, 19, &[5, 5], &[0], &[false]);
        assert_eq!(offset, 3);
        assert_eq!(gaps, vec![3]);
    }

    #[test]
    fn space_evenly_remainder() {
        // 3 children, leftover=10, slots=4: distribute_space(10,4) = base=2, rem=2 → [3,3,2,2]
        // offset=3, gaps: [0+3, 0+2] = [3, 2]
        let (offset, gaps) = call(Justify::SpaceEvenly, 10);
        assert_eq!(offset, 3);
        assert_eq!(gaps, vec![3, 2]);
    }

    #[test]
    fn space_around_half_edges() {
        // 3 children, leftover=12, slots=6: distribute_space(12,6) = [2,2,2,2,2,2]
        // offset = spaces[0] = 2  (half of between-gap)
        // gap[0] = spaces[1]+spaces[2] = 2+2 = 4
        // gap[1] = spaces[3]+spaces[4] = 2+2 = 4
        // trailing = spaces[5] = 2  (not stored, implicit)
        let (offset, gaps) = call(Justify::SpaceAround, 12);
        assert_eq!(offset, 2);
        assert_eq!(gaps, vec![4, 4]);
    }

    #[test]
    fn zero_leftover_all_variants() {
        let variants = [
            Justify::Start,
            Justify::Center,
            Justify::End,
            Justify::SpaceBetween,
            Justify::SpaceEvenly,
            Justify::SpaceAround,
        ];
        for variant in variants {
            let (offset, gaps) = call(variant, 0);
            assert_eq!(offset, 0, "offset for {variant:?}");
            assert_eq!(gaps, vec![0, 0], "gaps for {variant:?}");
        }
    }
}
