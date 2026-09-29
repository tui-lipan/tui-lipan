//! A translucent `Animated` crossfades with the glyphs it is drawn over, not only their colors.

use tui_lipan::TestBackend;
use tui_lipan::prelude::*;

const OLD_FG: Color = Color::Rgb(240, 240, 240);
const OLD_BG: Color = Color::Rgb(10, 10, 10);
const NEW_FG: Color = Color::Rgb(230, 230, 230);
const NEW_BG: Color = Color::Rgb(20, 20, 20);

/// `"ab"` underneath, and a layer drawing `new` over it at `opacity`.
struct Crossfade {
    new: &'static str,
    opacity: f32,
    fg_only: bool,
}

impl Component for Crossfade {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        let layer = |text: &'static str, fg: Color, bg: Color| {
            VStack::new()
                .style(Style::new().bg(bg))
                .child(Text::new(text).style(Style::new().fg(fg).bg(bg)))
        };
        ZStack::new()
            .child(layer("ab", OLD_FG, OLD_BG))
            .child(
                Animated::new(layer(self.new, NEW_FG, NEW_BG))
                    .opacity(self.opacity)
                    .opacity_fg_only(self.fg_only),
            )
            .into()
    }
}

/// Each cell's symbol and the red channel of its ink, where it has an RGB one.
fn capture(new: &'static str, opacity: f32, fg_only: bool) -> Vec<(String, Option<u8>)> {
    let mut backend = TestBackend::new(Crossfade {
        new,
        opacity,
        fg_only,
    });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 3,
        h: 1,
    });
    backend.render();
    let frame = backend.capture_frame();
    (0..3)
        .map(|x| {
            let cell = frame.cell(x, 0);
            let ink = match cell.fg {
                Color::Rgb(red, _, _) => Some(red),
                _ => None,
            };
            (cell.symbol.clone(), ink)
        })
        .collect()
}

fn symbols(cells: &[(String, Option<u8>)]) -> Vec<&str> {
    cells.iter().map(|(symbol, _)| symbol.as_str()).collect()
}

#[test]
fn the_underlay_glyph_shows_through_the_blank_cells_of_a_fading_layer() {
    let early = capture("x", 0.25, false);
    let late = capture("x", 0.75, false);
    assert_eq!(
        symbols(&early),
        ["a", "b", " "],
        "the old line still dominates"
    );
    assert_eq!(
        symbols(&late),
        ["x", "b", " "],
        "the new glyph wins its cell past half opacity; the old one fades on beside it"
    );
    assert!(
        early[1]
            .1
            .zip(late[1].1)
            .is_some_and(|(early, late)| early > late),
        "the old glyph fades out as the layer fades in: {:?} then {:?}",
        early[1].1,
        late[1].1
    );
    assert_eq!(symbols(&capture("x", 1.0, false)), ["x", " ", " "]);
}

#[test]
fn an_opaque_background_hides_the_underlay_glyphs() {
    assert_eq!(symbols(&capture("x", 0.25, true)), ["x", " ", " "]);
}

#[test]
fn a_wide_glyph_keeps_its_trailing_cell() {
    assert_eq!(symbols(&capture("漢", 0.75, false))[0], "漢");
    assert_ne!(
        symbols(&capture("漢", 0.75, false))[1],
        "b",
        "the old glyph must not split the wide one"
    );
    assert_eq!(
        symbols(&capture("漢", 0.25, false))[..2],
        ["a", "b"],
        "before the swap the old narrow glyphs own both cells"
    );
}

/// Two titled pages, the second fading in over the first at `opacity`.
struct FramedCrossfade {
    opacity: f32,
}

impl Component for FramedCrossfade {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        let page = |title: &'static str, bg: Color| {
            Frame::new()
                .header_left(title)
                .border(true)
                // A page covers the one beneath: its border replaces that page's title rather
                // than keeping it as a neighbour's.
                .border_merge_mode(BorderMergeMode::Replace)
                .style(Style::new().bg(bg))
                .child(Text::new(""))
        };
        ZStack::new()
            .child(page("Launcher", OLD_BG))
            .child(Animated::new(page("Session", NEW_BG)).opacity(self.opacity))
            .into()
    }
}

#[test]
fn a_longer_old_title_dissolves_under_the_new_border() {
    let mut backend = TestBackend::new(FramedCrossfade { opacity: 0.75 });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 14,
        h: 3,
    });
    backend.render();
    let frame = backend.capture_frame();
    let row: String = (0..14).map(|x| frame.cell(x, 0).symbol.clone()).collect();
    assert_eq!(
        row, "┌Session─────┐",
        "the new border wins past half opacity"
    );
    assert_ne!(
        frame.cell(8, 0).bg,
        OLD_BG,
        "the old title's cell blends too"
    );
}

struct Page(&'static str);

/// One keyed page at a time. A replaced page holds, whole, beneath its successor for 200ms.
struct HeldPage;

impl Component for HeldPage {
    type Message = &'static str;
    type Properties = ();
    type State = Page;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        Page("ab")
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        ctx.state.0 = msg;
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let text = ctx.state.0;
        ZStack::new()
            .child(
                Animated::new(Text::new(text).style(Style::new().fg(OLD_FG).bg(OLD_BG)))
                    .opacity(0.25)
                    .auto_exit(ExitAnimation::new(200).keep_opacity())
                    .key(text),
            )
            .into()
    }
}

#[test]
fn an_exit_with_nothing_to_animate_holds_for_its_duration() {
    let mut backend = TestBackend::new(HeldPage);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 3,
        h: 1,
    });
    backend.render();
    backend.dispatch("x").expect("dispatch");
    backend.render();
    let cell = |backend: &TestBackend<HeldPage>| backend.capture_frame().cell(1, 0).symbol.clone();
    assert_eq!(
        cell(&backend),
        "b",
        "the old page shows through the new one"
    );

    backend.advance(std::time::Duration::from_millis(100));
    assert_eq!(cell(&backend), "b", "and is still held halfway through");

    backend.advance(std::time::Duration::from_millis(200));
    assert_eq!(cell(&backend), " ", "then released");
}
