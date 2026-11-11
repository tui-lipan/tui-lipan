/// Width requirements and retention priority for one status-bar item.
///
/// Higher priorities survive space pressure first. Equal priorities retain source order.
/// Set `minimum` below `preferred` for content, such as a scrolling tab strip, that can
/// use a smaller viewport. A zero allocation means the item should be omitted entirely.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusBarItemSize {
    /// Natural width, in terminal cells.
    pub preferred: u16,
    /// Smallest usable width, in terminal cells.
    pub minimum: u16,
    /// Retention priority; larger values are more important.
    pub priority: u8,
}

impl StatusBarItemSize {
    /// Measure an element at its natural width, initially treating it as indivisible.
    pub fn measure(element: &crate::Element, priority: u8) -> Self {
        let preferred = crate::layout::measure::min_size_constrained(element, None, None).0;
        Self {
            preferred,
            minimum: preferred,
            priority,
        }
    }
}

pub(super) fn allocate_items(items: &[StatusBarItemSize], available: u16, gap: u16) -> Vec<u16> {
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by_key(|&index| std::cmp::Reverse(items[index].priority));
    let mut widths = vec![0; items.len()];
    let mut remaining = available;
    let mut retained = false;
    for &index in &order {
        let item = items[index];
        let minimum = item.minimum.max(1).min(item.preferred);
        let spacing = if retained { gap } else { 0 };
        let cost = u32::from(minimum) + u32::from(spacing);
        if minimum > 0 && cost <= u32::from(remaining) {
            widths[index] = minimum;
            remaining -= cost as u16;
            retained = true;
        }
    }
    for index in order {
        if widths[index] > 0 {
            let extra = remaining.min(items[index].preferred - widths[index]);
            widths[index] += extra;
            remaining -= extra;
        }
    }
    widths
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StatusBar;

    #[test]
    fn priority_keeps_whole_items_and_only_charges_visible_gaps() {
        let items = [
            StatusBarItemSize {
                preferred: 10,
                minimum: 10,
                priority: 10,
            },
            StatusBarItemSize {
                preferred: 7,
                minimum: 7,
                priority: 200,
            },
            StatusBarItemSize {
                preferred: 8,
                minimum: 8,
                priority: 100,
            },
        ];
        assert_eq!(StatusBar::allocate_items(&items, 26, 1), [0, 7, 8]);
        assert_eq!(StatusBar::allocate_items(&items, 16, 1), [0, 7, 8]);
        assert_eq!(StatusBar::allocate_items(&items, 15, 1), [0, 7, 0]);
        assert_eq!(StatusBar::allocate_items(&items, 6, 1), [0, 0, 0]);
    }

    #[test]
    fn scrolling_item_gets_the_remaining_width_after_minimums_fit() {
        let items = [
            StatusBarItemSize {
                preferred: 6,
                minimum: 6,
                priority: 10,
            },
            StatusBarItemSize {
                preferred: 30,
                minimum: 7,
                priority: 100,
            },
        ];
        assert_eq!(StatusBar::allocate_items(&items, 40, 1), [6, 30]);
        assert_eq!(StatusBar::allocate_items(&items, 20, 1), [6, 13]);
        assert_eq!(StatusBar::allocate_items(&items, 10, 1), [0, 10]);
        assert_eq!(StatusBar::allocate_items(&items, 6, 1), [6, 0]);
    }

    #[test]
    fn equal_priorities_keep_source_order_and_handle_large_gaps() {
        let item = StatusBarItemSize {
            preferred: 3,
            minimum: 3,
            priority: 1,
        };
        assert_eq!(StatusBar::allocate_items(&[item; 3], 7, 1), [3, 3, 0]);
        assert_eq!(
            StatusBar::allocate_items(&[item; 2], u16::MAX, u16::MAX),
            [3, 0]
        );
        assert_eq!(StatusBar::allocate_items(&[], 80, 1), Vec::<u16>::new());
    }

    #[test]
    fn allocations_never_exceed_the_budget() {
        let elements: [crate::Element; 3] = [
            crate::widgets::Text::new("optional").into(),
            crate::widgets::Text::new("long scrolling navigation").into(),
            crate::widgets::Text::new("mode").into(),
        ];
        let mut items = [
            StatusBarItemSize::measure(&elements[0], 10),
            StatusBarItemSize::measure(&elements[1], 100),
            StatusBarItemSize::measure(&elements[2], 200),
        ];
        items[1].minimum = 7;
        for gap in [0, 1, 3] {
            for available in 0..80 {
                let widths = StatusBar::allocate_items(&items, available, gap);
                let count = widths.iter().filter(|&&width| width > 0).count();
                let used = widths.iter().map(|&w| u32::from(w)).sum::<u32>()
                    + u32::from(gap) * count.saturating_sub(1) as u32;
                assert!(used <= u32::from(available));
                for (item, width) in items.iter().zip(widths) {
                    assert!(width == 0 || (item.minimum..=item.preferred).contains(&width));
                }
            }
        }
    }

    #[test]
    fn centered_side_budgets_include_gaps_and_odd_width_rounding() {
        assert_eq!(StatusBar::side_widths(80, 14, 1), (32, 32));
        assert_eq!(StatusBar::side_widths(81, 14, 1), (32, 33));
        assert_eq!(StatusBar::side_widths(10, 14, 1), (0, 0));
    }
}
