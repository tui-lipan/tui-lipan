//! `Select` forwards trigger padding and label alignment to its button.

use tui_lipan::TestBackend;
use tui_lipan::prelude::*;

#[derive(Clone, Copy)]
struct Filled {
    padding: Option<u16>,
    align: Option<Align>,
}

impl Component for Filled {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        let mut select = Select::new()
            .options(["one", "two"])
            .selected(Some(0))
            .width(Length::Px(12))
            .button_variant(ButtonVariant::Filled);
        if let Some(padding) = self.padding {
            select = select.button_padding(padding);
        }
        if let Some(align) = self.align {
            select = select.button_align(align);
        }
        select.into()
    }
}

fn first_line(padding: Option<u16>, align: Option<Align>) -> String {
    let mut backend = TestBackend::new(Filled { padding, align });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 12,
        h: 1,
    });
    backend.render();
    backend.capture_frame().to_lines().remove(0)
}

#[test]
fn the_trigger_defaults_to_padded_and_centered() {
    let line = first_line(None, None);
    assert_eq!(line.trim(), "one", "{line:?}");
    assert!(line.starts_with("    "), "{line:?}");
}

#[test]
fn button_padding_and_align_reach_the_trigger() {
    let line = first_line(Some(0), None);
    assert!(
        line.starts_with("    one"),
        "centered without padding: {line:?}"
    );
    let line = first_line(Some(0), Some(Align::Start));
    assert!(line.starts_with("one"), "{line:?}");
}
