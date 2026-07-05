use crate::style::Length;

fn resolve_requested(length: Length, available: Option<u16>) -> u16 {
    match length {
        Length::Auto => 0,
        Length::Px(px) => px,
        Length::Percent(percent) => available
            .map(|available| {
                let percent = percent.min(100);
                ((available as u32).saturating_mul(percent as u32) / 100).min(u16::MAX as u32)
                    as u16
            })
            .unwrap_or(0),
        Length::Flex(_) => available.unwrap_or(0),
    }
}

pub(crate) fn measure_canvas(
    canvas: &super::Canvas,
    max_w: Option<u16>,
    max_h: Option<u16>,
) -> (u16, u16) {
    (
        resolve_requested(canvas.width, max_w),
        resolve_requested(canvas.height, max_h),
    )
}
