use crate::widgets::Chart;

pub fn measure_chart(chart: &Chart) -> (u16, u16) {
    let max_points = chart
        .series
        .iter()
        .map(|series| series.data.len())
        .max()
        .unwrap_or(0)
        .max(8);

    let mut w = (max_points as u16).min(120);
    if chart.y_axis.show {
        w = w.saturating_add(8);
    }
    if chart.show_legend {
        w = w.saturating_add(4);
    }

    let mut h = 6u16;
    if chart.x_axis.show {
        h = h.saturating_add(1);
    }
    if chart.show_legend {
        h = h.saturating_add(1);
    }

    w = w.saturating_add(chart.padding.horizontal());
    h = h.saturating_add(chart.padding.vertical());

    if chart.border {
        w = w.saturating_add(2);
        h = h.saturating_add(2);
    }

    (w, h)
}
