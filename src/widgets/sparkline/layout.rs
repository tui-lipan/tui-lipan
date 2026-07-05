use super::{Sparkline, SparklineVariant};
use crate::style::Length;

pub fn measure_sparkline(spark: &Sparkline) -> (u16, u16) {
    let data_len = spark.data.len();
    if data_len == 0 {
        return (0, 0);
    }

    let points = if let Some(max) = spark.max_points {
        max.min(data_len)
    } else {
        data_len
    };

    let width = match spark.variant {
        SparklineVariant::Bars | SparklineVariant::Line => points as u16,
        SparklineVariant::Braille => points.div_ceil(2) as u16,
    };

    let height = spark.chart_height;

    let w = match spark.width {
        Length::Px(px) => px,
        Length::Flex(_) | Length::Percent(_) => 0,
        Length::Auto => width,
    };
    let h = match spark.height {
        Length::Px(px) => px,
        Length::Flex(_) | Length::Percent(_) => 0,
        Length::Auto => height,
    };

    (w, h)
}
