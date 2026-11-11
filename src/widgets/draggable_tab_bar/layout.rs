use super::DraggableTabBar;

pub fn measure_draggable_tab_bar(bar: &DraggableTabBar) -> (u16, u16) {
    let mut w = DraggableTabBar::content_width_with_options(&bar.tabs, &bar.display_options());
    let mut h = 1usize;

    w = w.saturating_add(bar.padding.horizontal() as usize);
    h = h.saturating_add(bar.padding.vertical() as usize);

    if bar.border {
        w = w.saturating_add(2);
        h = h.saturating_add(2);
    }

    (
        w.min(u16::MAX as usize) as u16,
        h.min(u16::MAX as usize) as u16,
    )
}

#[cfg(test)]
mod tests {
    use crate::core::element::Element;
    use crate::layout::measure::min_size_constrained;
    use crate::style::{Length, Span};

    use super::*;
    use crate::widgets::DraggableTab;

    #[test]
    fn auto_width_tracks_changed_tab_content_through_the_measure_cache() {
        let measure = |tab: DraggableTab| {
            let element: Element = DraggableTabBar::new()
                .tab(tab)
                .width(Length::Auto)
                .show_close_buttons(true)
                .show_file_icons(false)
                .position_label(|slot, label| format!("{}:{label}", slot + 1).into())
                .into();
            min_size_constrained(&element, Some(100), Some(1)).0
        };
        assert_eq!(measure(DraggableTab::new("x")), 5);
        assert_eq!(measure(DraggableTab::new("docs")), 8);
        assert_eq!(measure(DraggableTab::new("docs").icon(Span::new("*"))), 10);
        assert_eq!(
            measure(DraggableTab::new("docs").right_badge(Span::new("OK"))),
            11
        );
        assert_eq!(measure(DraggableTab::new("docs").closeable(true)), 10);
        assert_eq!(measure(DraggableTab::new("x")), 5);
    }
}
