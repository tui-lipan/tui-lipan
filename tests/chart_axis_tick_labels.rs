//! `ChartAxis::tick_labels` replaces the numeric sample-index endpoints with
//! domain labels, spreads them across the axis, and thins them out rather than
//! overprinting when the plot is too narrow to hold them all.

use tui_lipan::TestBackend;
use tui_lipan::prelude::*;

struct Demo {
    x_labels: Vec<&'static str>,
    y_labels: Vec<&'static str>,
}

impl Component for Demo {
    type Message = ();
    type Properties = ();
    type State = ();
    fn create_state(&self, _: &Self::Properties) -> Self::State {}
    fn update(&mut self, _: Self::Message, _: &mut Context<Self>) -> Update {
        Update::none()
    }
    fn view(&self, _ctx: &Context<Self>) -> Element {
        let samples: Vec<f64> = (0..60).map(f64::from).collect();
        let mut chart = Chart::new()
            .series([ChartSeries::new("s", samples)])
            .show_grid(false)
            .show_legend(false)
            .height(Length::Flex(1));
        if !self.x_labels.is_empty() {
            chart = chart.x_axis(ChartAxis::new().tick_labels(self.x_labels.clone()));
        }
        if !self.y_labels.is_empty() {
            chart = chart.y_axis(ChartAxis::new().tick_labels(self.y_labels.clone()));
        }
        chart.into()
    }
}

fn render(x_labels: &[&'static str], y_labels: &[&'static str], w: u16, h: u16) -> Vec<String> {
    let mut backend = TestBackend::new(Demo {
        x_labels: x_labels.to_vec(),
        y_labels: y_labels.to_vec(),
    });
    backend.set_viewport(Rect { x: 0, y: 0, w, h });
    backend.render();
    backend
        .capture_frame()
        .to_fixed_grid_lines()
        .into_iter()
        .map(|line| line.trim_end().to_string())
        .collect()
}

#[test]
fn without_tick_labels_the_x_axis_still_shows_sample_indices() {
    let grid = render(&[], &[], 60, 12);
    let axis = grid.last().expect("axis row");

    assert!(axis.trim_start().starts_with('0'), "{axis:?}");
    assert!(axis.trim_end().ends_with("59"), "{axis:?}");
}

#[test]
fn x_tick_labels_replace_the_indices_and_anchor_both_ends() {
    let labels = ["22:40:00", "22:40:15", "22:40:30", "22:40:45", "22:40:59"];
    let grid = render(&labels, &[], 80, 12);
    let axis = grid.last().expect("axis row");

    for label in labels {
        assert!(axis.contains(label), "missing {label} in {axis:?}");
    }
    assert!(axis.trim_end().ends_with("22:40:59"), "{axis:?}");

    let first = axis.find("22:40:00").expect("first label");
    let plot_start = grid[0].find(|c: char| c != ' ').unwrap_or(0);
    assert!(first >= plot_start, "first label ran into the y gutter");
}

#[test]
fn colliding_x_tick_labels_are_skipped_instead_of_overprinted() {
    let labels = ["22:40:00", "22:40:15", "22:40:30", "22:40:45", "22:40:59"];
    let grid = render(&labels, &[], 40, 12);
    let axis = grid.last().expect("axis row");

    let kept = labels.iter().filter(|label| axis.contains(*label)).count();
    assert!(
        kept < labels.len(),
        "narrow axis kept every label: {axis:?}"
    );
    assert!(kept >= 2, "narrow axis dropped too much: {axis:?}");
    // Overprinting would splice two labels into one run of digits and colons.
    assert!(!axis.contains(":000"), "labels overlapped: {axis:?}");
}

#[test]
fn y_tick_labels_run_bottom_to_top_in_the_axis_gutter() {
    let grid = render(&[], &["low", "mid", "high"], 60, 12);

    let row_of = |needle: &str| {
        grid.iter()
            .position(|line| line.contains(needle))
            .unwrap_or_else(|| panic!("missing {needle} in {grid:#?}"))
    };

    assert!(
        row_of("high") < row_of("mid") && row_of("mid") < row_of("low"),
        "y labels are not ordered high-to-low top-down: {grid:#?}"
    );
    // The default numeric endpoints must be gone, not drawn underneath.
    assert!(
        !grid.iter().any(|line| line.contains("59.00")),
        "numeric y endpoints survived: {grid:#?}"
    );
}
