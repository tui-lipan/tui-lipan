//! A toast's width cap decides where its message wraps.
use std::time::Duration;
use tui_lipan::TestBackend;
use tui_lipan::prelude::*;

const WIDTH: u16 = 40;
const MESSAGE: &str = "a long message that wraps across several lines of the toast";

struct Host {
    toast: Toast,
}

impl Component for Host {
    type Message = ();
    type Properties = ();
    type State = ();
    fn create_state(&self, _: &()) {}
    fn update(&mut self, _: (), ctx: &mut Context<Self>) -> Update {
        ctx.toast().push(self.toast.clone());
        Update::full()
    }
    fn view(&self, _: &Context<Self>) -> Element {
        Text::new("Background").into()
    }
}

/// The rows the toast's frame covers, border included.
fn toast_rows(toast: Toast) -> Vec<String> {
    let mut backend = TestBackend::new(Host { toast });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: WIDTH,
        h: 16,
    });
    backend.render();
    backend.dispatch(()).unwrap();
    backend.advance(Duration::from_millis(500));
    let lines = backend.capture_frame().to_fixed_grid_lines();
    let top = lines.iter().position(|line| line.contains('╭')).unwrap();
    let bottom = lines.iter().position(|line| line.contains('╰')).unwrap();
    lines[top..=bottom]
        .iter()
        .map(|line| line.trim().to_string())
        .collect()
}

#[test]
fn a_percentage_max_width_wraps_the_message_at_that_share_of_the_screen() {
    let rows = toast_rows(
        Toast::new(MESSAGE)
            .duration(10.0)
            .max_width(Length::Percent(50)),
    );

    assert_eq!(rows[0].chars().count(), usize::from(WIDTH / 2));
    // Every row between the borders carries text; the frame is as tall as the wrap.
    assert!(
        rows[1..rows.len() - 1]
            .iter()
            .all(|row| !row.trim_matches(['│', ' ']).is_empty()),
        "{rows:#?}"
    );
}

#[test]
fn a_pixel_max_width_wraps_the_message_at_that_width() {
    let rows = toast_rows(Toast::new(MESSAGE).duration(10.0).max_width(Length::Px(16)));

    assert_eq!(rows[0].chars().count(), 16);
}
