use tui_lipan::prelude::*;
use tui_lipan::{CapturedFrame, TestBackend};

const W: u16 = 26;

#[derive(Clone, Copy)]
struct SpinnerList {
    frame: usize,
}

impl Component for SpinnerList {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        List::new()
            .items([
                ListItem::new("Deploy")
                    .description("building")
                    .description_spinner(Spinner::new().frame(self.frame)),
                ListItem::new("Index")
                    .description("done")
                    .description_spinner(Spinner::new().frame(self.frame))
                    .description_spinner_position(ListSymbolPosition::Right),
                ListItem::new("Sync")
                    .description("queued")
                    .label_spinner(Spinner::new().frame(self.frame)),
                ListItem::new("A very long row label that cannot fit")
                    .description("truncating")
                    .description_spinner(Spinner::new().frame(self.frame)),
            ])
            .symbol_column(false)
            .width(Length::Px(W))
            .height(Length::Px(4))
            .focusable(false)
            .into()
    }
}

fn rows(frame: usize) -> Vec<String> {
    let mut backend = TestBackend::new(SpinnerList { frame });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: W,
        h: 4,
    });
    backend.render();
    let captured: CapturedFrame = backend.capture_frame();
    (0..4)
        .map(|y| (0..W).map(|x| captured.cell(x, y).symbol.clone()).collect())
        .collect()
}

#[test]
fn description_spinner_renders_on_the_requested_side_of_the_description() {
    let rows = rows(0);

    assert_eq!(rows[0], "Deploy          ⠋ building");
    assert_eq!(rows[1], "Index               done ⠋");
    assert_eq!(rows[2], "⠋ Sync              queued");
}

#[test]
fn spinner_cells_never_overlap_the_description_text() {
    // A wide spinner must take its width out of the label budget, not paint over the
    // description it sits next to.
    let rows = rows(0);
    let long = &rows[3];

    assert!(long.ends_with("⠋ truncating"), "row = {long:?}");
    assert!(long.starts_with("A very long"), "row = {long:?}");
}

#[test]
fn animating_a_frame_leaves_every_other_cell_untouched() {
    // The whole point of the reserved slot: text must not re-truncate or shift as the
    // glyph cycles.
    let first = rows(0);
    let later = rows(4);

    for (a, b) in first.iter().zip(later.iter()) {
        let stripped_a: String = a.chars().filter(|c| !"⠋⠼".contains(*c)).collect();
        let stripped_b: String = b.chars().filter(|c| !"⠋⠼".contains(*c)).collect();
        assert_eq!(stripped_a, stripped_b, "\n{a:?}\n{b:?}");
    }
    assert_ne!(first, later, "the spinner glyph itself should have changed");
}

#[derive(Clone, Copy)]
struct SpinnerPalette {
    placement: DescriptionPlacement,
}

impl Component for SpinnerPalette {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        SearchPalette::new()
            .items([SearchItem::new("Deploy", 0).description(
                ItemDescription::new()
                    .left("building")
                    .spinner(Spinner::new().frame(0)),
            )])
            .description_placement(self.placement)
            .width(Length::Px(W))
            .height(Length::Px(4))
            .into()
    }
}

fn palette_rows(placement: DescriptionPlacement) -> Vec<String> {
    let mut backend = TestBackend::new(SpinnerPalette { placement });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: W,
        h: 4,
    });
    backend.render();
    let captured: CapturedFrame = backend.capture_frame();
    (0..4)
        .map(|y| {
            (0..W)
                .map(|x| captured.cell(x, y).symbol.clone())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

#[test]
fn palette_description_spinner_follows_the_description_placement() {
    // Inline shares one line with the label, so the slot anchors to the row.
    assert_eq!(
        palette_rows(DescriptionPlacement::Inline)[2],
        "> ⠋ Deploy - building"
    );
    // Right has its own description column.
    assert_eq!(
        palette_rows(DescriptionPlacement::Right)[2],
        "> Deploy        ⠋ building"
    );
    // Above and Below put it beside the description on the description's own line.
    assert_eq!(palette_rows(DescriptionPlacement::Above)[2], "  ⠋ building");
    assert_eq!(palette_rows(DescriptionPlacement::Below)[3], "  ⠋ building");
}

#[test]
fn palette_renders_exactly_one_description_spinner_per_row() {
    for placement in [
        DescriptionPlacement::Inline,
        DescriptionPlacement::Right,
        DescriptionPlacement::Above,
        DescriptionPlacement::Below,
    ] {
        let glyphs = palette_rows(placement).join("").matches('⠋').count();
        assert_eq!(glyphs, 1, "{placement:?} drew {glyphs} spinners");
    }
}
