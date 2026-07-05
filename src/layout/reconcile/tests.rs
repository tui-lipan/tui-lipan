fn compute_max_row_offset(
    content_height: u16,
    viewport_height: u16,
    show_scroll_indicators: bool,
) -> usize {
    let mut base_max = content_height.saturating_sub(viewport_height);
    if show_scroll_indicators && content_height > viewport_height && viewport_height > 0 {
        base_max = base_max.saturating_add(1);
    }
    base_max as usize
}

#[test]
fn test_row_max_offset_simple() {
    assert_eq!(compute_max_row_offset(5, 10, false), 0);
    assert_eq!(compute_max_row_offset(10, 10, false), 0);
    assert_eq!(compute_max_row_offset(15, 10, false), 5);
    assert_eq!(compute_max_row_offset(20, 10, false), 10);
}

#[test]
fn test_row_max_offset_with_indicators() {
    assert_eq!(compute_max_row_offset(5, 10, true), 0);
    assert_eq!(compute_max_row_offset(10, 10, true), 0);
    assert_eq!(compute_max_row_offset(15, 10, true), 6);
    assert_eq!(compute_max_row_offset(20, 10, true), 11);
}

#[test]
fn test_row_max_offset_zero_viewport() {
    assert_eq!(compute_max_row_offset(10, 0, false), 10);
    assert_eq!(compute_max_row_offset(10, 0, true), 10);
}
