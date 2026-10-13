//! How a palette shares a row between its label and a right-aligned description when it builds
//! the rows itself: the gap it holds between them, and the share a label keeps when the
//! description wins.

use tui_lipan::prelude::*;
use tui_lipan::{CapturedFrame, TestBackend};

const W: u16 = 32;
const H: u16 = 6;

#[derive(Clone)]
struct Palette {
    label: &'static str,
    description: &'static str,
    gap: u16,
    description_wins: bool,
    min_label: Option<Length>,
}

impl Palette {
    fn new(label: &'static str, description: &'static str) -> Self {
        Self {
            label,
            description,
            gap: 0,
            description_wins: false,
            min_label: None,
        }
    }
}

impl Component for Palette {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        let mut palette = SearchPalette::new()
            .items([SearchItem::new(self.label, 0)
                .description(ItemDescription::new().right(self.description))])
            .description_placement(DescriptionPlacement::Right)
            .primary_truncate_description_first(!self.description_wins)
            .primary_description_gap(self.gap)
            .width(Length::Px(W))
            .height(Length::Px(H));
        if let Some(width) = self.min_label {
            palette = palette.primary_min_label_width(width);
        }
        palette.into()
    }
}

/// The rendered result row: the selected one, behind its `> ` marker.
fn row(palette: Palette) -> String {
    let mut backend = TestBackend::new(palette);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: W,
        h: H,
    });
    backend.render();
    let captured: CapturedFrame = backend.capture_frame();
    (0..H)
        .map(|y| {
            (0..W)
                .map(|x| captured.cell(x, y).symbol.clone())
                .collect::<String>()
        })
        .find_map(|line| line.strip_prefix("> ").map(str::to_string))
        .expect("a selected result row")
}

const LONG: &str = "codex-reviewing-the-focus-path";
const TIGHT: &str = "abcdefghijklmnopqrstuvwxy";
const TIGHTER: &str = "abcdefghijklmnopqrstuvwxyz0";

#[test]
fn without_a_gap_rows_keep_the_palette_s_single_space() {
    assert_eq!(
        row(Palette::new("codex", "idle")),
        "codex                     idle"
    );
    assert_eq!(
        row(Palette::new(TIGHTER, "idle")),
        "abcdefghijklmnopqrstuvwxyz0 i…"
    );
}

#[test]
fn a_gap_holds_the_description_off_the_label() {
    let gap = |label| Palette {
        gap: 2,
        ..Palette::new(label, "idle")
    };
    assert_eq!(row(gap("codex")), "codex                     idle");
    assert_eq!(row(gap(TIGHT)), "abcdefghijklmnopqrstuvwxy  id…");
}

#[test]
fn with_a_gap_a_description_cut_to_a_bare_ellipsis_is_hidden() {
    let row = row(Palette {
        gap: 2,
        ..Palette::new(TIGHTER, "idle")
    });
    assert_eq!(row, "abcdefghijklmnopqrstuvwxyz0   ");
}

#[test]
fn a_winning_description_leaves_the_label_its_minimum_share() {
    let wins = |description, min_label| Palette {
        gap: 2,
        description_wins: true,
        min_label,
        ..Palette::new(LONG, description)
    };
    // Without a minimum the label is all but gone.
    assert_eq!(
        row(wins("working-on-the-picker-row", None)),
        "co…  working-on-the-picker-row"
    );
    assert_eq!(
        row(wins("working-on-the-picker-row", Some(Length::Percent(40)))),
        "codex-revie…  working-on-the-…"
    );
    // A description that fits is never cut for it.
    assert_eq!(
        row(wins("idle", Some(Length::Percent(40)))),
        "codex-reviewing-the-foc…  idle"
    );
}
