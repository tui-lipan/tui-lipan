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
