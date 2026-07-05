use unicode_width::UnicodeWidthStr;

use super::{ProgressBar, ProgressStyle, ProgressTextPosition};

pub fn measure_progress_bar(progress: &ProgressBar) -> (u16, u16) {
    let mut w = 10usize;
    let mut h = 1usize;

    let mut pct_position = progress.percentage_position;
    if !matches!(progress.progress_style, ProgressStyle::Block)
        && matches!(pct_position, ProgressTextPosition::Middle)
    {
        pct_position = ProgressTextPosition::Right;
    }
    let mut label_position = progress.label_position;
    if !matches!(progress.progress_style, ProgressStyle::Block)
        && matches!(label_position, ProgressTextPosition::Middle)
    {
        label_position = ProgressTextPosition::Right;
    }

    let percentage_width = 4usize; // "100%"

    let pct_vertical = progress.show_percentage
        && matches!(
            pct_position,
            ProgressTextPosition::Above | ProgressTextPosition::Below
        );
    let label_vertical = progress.label.is_some()
        && matches!(
            label_position,
            ProgressTextPosition::Above | ProgressTextPosition::Below
        );
    let same_vertical_line = pct_vertical && label_vertical && pct_position == label_position;

    if progress.show_percentage {
        match pct_position {
            ProgressTextPosition::Left | ProgressTextPosition::Right => {
                w = w.saturating_add(percentage_width + 1);
            }
            ProgressTextPosition::Above | ProgressTextPosition::Below => h = h.saturating_add(1),
            ProgressTextPosition::Middle => {}
        }
    }
    if let Some(label) = &progress.label {
        match label_position {
            ProgressTextPosition::Left | ProgressTextPosition::Right => {
                w = w.saturating_add(UnicodeWidthStr::width(label.as_str()).saturating_add(1));
            }
            ProgressTextPosition::Above | ProgressTextPosition::Below => {
                if !same_vertical_line {
                    h = h.saturating_add(1);
                }
            }
            ProgressTextPosition::Middle => {}
        }
    }

    w = w.saturating_add(progress.padding.horizontal() as usize);
    h = h.saturating_add(progress.padding.vertical() as usize);

    let w = w.min(u16::MAX as usize) as u16;
    let h = h.min(u16::MAX as usize) as u16;
    (w, h)
}

#[cfg(test)]
mod tests {
    use super::{ProgressBar, ProgressTextPosition, measure_progress_bar};

    #[test]
    fn side_label_consumes_width() {
        let progress = ProgressBar::new(0.5).label("done");

        assert_eq!(measure_progress_bar(&progress), (15, 1));
    }

    #[test]
    fn vertical_label_consumes_height_not_width() {
        let progress = ProgressBar::new(0.5)
            .label("done")
            .label_position(ProgressTextPosition::Below);

        assert_eq!(measure_progress_bar(&progress), (10, 2));
    }

    #[test]
    fn same_vertical_percentage_and_label_share_height() {
        let progress = ProgressBar::new(0.5)
            .show_percentage(true)
            .percentage_position(ProgressTextPosition::Above)
            .label("done")
            .label_position(ProgressTextPosition::Above);

        assert_eq!(measure_progress_bar(&progress), (10, 2));
    }
}
